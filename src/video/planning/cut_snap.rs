//! Acoustic refinement of dialogue cut points.
//!
//! ASR word timestamps are only accurate to a few tens of milliseconds and are
//! frequently worse at word edges: Granite emits only an *end* marker per word,
//! so a word's start is simply the previous word's end. Cutting exactly at (or a
//! fixed pad around) those stamps therefore clips the tail of a kept word or lets
//! the onset of a deleted neighbour leak into the render.
//!
//! This module looks at the audio that is actually rendered and moves each cut to
//! the quietest nearby point within 60 ms of its target. Shared cuts may
//! intentionally enter neighbouring edge words, bounded by half their duration;
//! split and outer cuts stay outside the cue's words.

use std::io::Read;

use anyhow::{Context, Result, ensure};

use crate::video::render::timeline::SourceRange;
use crate::video::support::transcript::TranscriptCue;

/// Resolution of the loudness envelope.
const HOP_SECONDS: f64 = 0.005;

/// One-pole high-pass pole (~80 Hz at 16 kHz) so rumble and DC offset do not
/// hide the dips between words.
const HIGH_PASS_POLE: f32 = 0.97;

const POWER_FLOOR: f32 = 1e-10;

/// Loudness (dB) we are willing to give up per second a cut moves away from its
/// target. 0.1 dB/ms means a dip must be ~6 dB deeper to justify moving 60 ms,
/// which keeps cuts close to the stamp on silent plateaus and lets a real valley
/// win in continuous speech.
const DISTANCE_PENALTY_DB_PER_SECOND: f32 = 100.0;

/// How far a cut may move from its target in either direction.
pub(super) const SNAP_WINDOW_SECONDS: f64 = 0.06;

/// Short-time loudness of a mono recording on a fixed 5 ms grid.
#[derive(Debug, Clone)]
pub struct AudioEnvelope {
    db: Vec<f32>,
}

impl AudioEnvelope {
    /// Build an envelope from mono signed 16-bit little-endian PCM.
    pub fn from_s16le_reader<R: Read>(mut reader: R, sample_rate: u32) -> Result<Self> {
        let hop_samples = ((f64::from(sample_rate) * HOP_SECONDS).round() as usize).max(1);

        let mut powers: Vec<f32> = Vec::new();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut carry: Option<u8> = None;
        let (mut previous_input, mut previous_output) = (0.0f32, 0.0f32);
        let (mut accumulated, mut in_hop) = (0.0f32, 0usize);

        loop {
            let read = reader
                .read(&mut buffer)
                .context("Failed to read PCM audio for cut refinement")?;
            if read == 0 {
                break;
            }
            let mut bytes = &buffer[..read];
            let mut feed = |low: u8, high: u8| {
                let input = f32::from(i16::from_le_bytes([low, high])) / 32768.0;
                let output = input - previous_input + HIGH_PASS_POLE * previous_output;
                previous_input = input;
                previous_output = output;
                accumulated += output * output;
                in_hop += 1;
                if in_hop == hop_samples {
                    powers.push(accumulated / hop_samples as f32);
                    accumulated = 0.0;
                    in_hop = 0;
                }
            };
            if let Some(low) = carry.take() {
                feed(low, bytes[0]);
                bytes = &bytes[1..];
            }
            let (pairs, remainder) = bytes.as_chunks::<2>();
            for [low, high] in pairs {
                feed(*low, *high);
            }
            if let [leftover] = remainder {
                carry = Some(*leftover);
            }
        }

        // Smooth over three hops (15 ms) so a single noisy frame cannot win.
        let db = (0..powers.len())
            .map(|index| {
                let lo = index.saturating_sub(1);
                let hi = (index + 1).min(powers.len() - 1);
                let mean = powers[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32;
                10.0 * (mean + POWER_FLOOR).log10()
            })
            .collect();
        Ok(Self { db })
    }

    pub fn duration(&self) -> f64 {
        self.db.len() as f64 * HOP_SECONDS
    }

    /// Intersect boundary constraints with the maximum movement from `target`.
    /// Reject empty intersections before the generic envelope search can fall
    /// back to a target outside the boundary constraints.
    fn snapped_boundary_point(&self, target: f64, lo: f64, hi: f64) -> Result<f64> {
        let lo = lo.max(target - SNAP_WINDOW_SECONDS);
        let hi = hi.min(target + SNAP_WINDOW_SECONDS);
        ensure!(
            lo <= hi,
            "No valid cut within +/-{SNAP_WINDOW_SECONDS} s of target {target}: bounds [{lo}, {hi}]"
        );
        Ok(self.quietest_point(target, lo, hi).clamp(lo, hi))
    }

    /// Return the time in `[lo, hi]` that best balances "quiet" against "close
    /// to `target`". Falls back to `target` when the bounds are empty or lie
    /// outside the recording, so a short envelope can never push a cut around.
    pub fn quietest_point(&self, target: f64, lo: f64, hi: f64) -> f64 {
        let lo = lo.max(0.0);
        let hi = hi.min(self.duration());
        if self.db.is_empty() || lo > hi {
            return target;
        }

        // Frame `i` covers [i, i + 1) hops and is represented by its centre.
        let first = ((lo / HOP_SECONDS - 0.5).ceil().max(0.0)) as usize;
        let last = (((hi / HOP_SECONDS - 0.5).floor().max(0.0)) as usize).min(self.db.len() - 1);
        if first > last || frame_centre(first) > hi || frame_centre(last) < lo {
            return target.clamp(lo, hi);
        }

        let mut best_index = first;
        let mut best_cost = f32::INFINITY;
        for index in first..=last {
            let time = frame_centre(index);
            let cost =
                self.db[index] + DISTANCE_PENALTY_DB_PER_SECOND * (time - target).abs() as f32;
            if cost < best_cost {
                best_cost = cost;
                best_index = index;
            }
        }

        // Keep the stamp itself when it already sits inside the winning frame,
        // so a silent plateau does not shift cuts by quantisation alone.
        let centre = frame_centre(best_index);
        target.clamp(
            (centre - HOP_SECONDS / 2.0).max(lo),
            (centre + HOP_SECONDS / 2.0).min(hi),
        )
    }
}

fn frame_centre(index: usize) -> f64 {
    (index as f64 + 0.5) * HOP_SECONDS
}

/// Where two neighbouring transcript cues are separated.
enum Boundary {
    /// Cues are close enough to touch: both sides must use the identical cut so
    /// that two kept neighbours stay sample-contiguous (no fade, no gap, no
    /// repeated audio).
    Shared(f64),
    /// Real silence between the cues: each side keeps its own padded cut.
    Split { end: f64, start: f64 },
}

/// Half the duration of the cue's first or last word. Each shared cut may
/// enter an edge word by at most this much; alignment also checks both edges
/// together so a single-word cue cannot be trimmed away from both sides.
fn half_edge_word(cue: &TranscriptCue, last: bool) -> f64 {
    let word = if last {
        cue.words.last()
    } else {
        cue.words.first()
    };
    word.map_or(0.0, |word| {
        word.end.saturating_sub(word.start).as_secs_f64() / 2.0
    })
}

fn boundary_between(
    cues: &[TranscriptCue],
    left_idx: usize,
    padding: f64,
    envelope: &AudioEnvelope,
) -> Result<Boundary> {
    let left = &cues[left_idx];
    let right = &cues[left_idx + 1];
    let left_end = left.end.as_secs_f64();
    let right_start = right.start.as_secs_f64();
    let mid = (left_end + right_start) / 2.0;

    if right_start - left_end <= 2.0 * padding {
        let lo = left_end - half_edge_word(left, true);
        let hi = right_start + half_edge_word(right, false);
        ensure!(
            lo <= hi,
            "Contradictory shared word half bounds for source `{}` between cue #{} and cue #{}: [{lo}, {hi}]",
            left.source_id,
            left.cue_id,
            right.cue_id
        );
        let cut = envelope
            .snapped_boundary_point(mid, lo, hi)
            .with_context(|| {
                format!(
                    "Cannot snap shared boundary for source `{}` between cue #{} and cue #{}",
                    left.source_id, left.cue_id, right.cue_id
                )
            })?;
        Ok(Boundary::Shared(cut))
    } else {
        Ok(Boundary::Split {
            end: envelope.snapped_boundary_point(left_end + padding, left_end, mid)?,
            start: envelope.snapped_boundary_point(right_start - padding, mid, right_start)?,
        })
    }
}

/// Source range for `cues[idx]` with both edges snapped to the quietest point
/// within 60 ms of the padded word stamps (or the midpoint for shared cuts).
/// Shared cuts may enter neighbouring edge words by up to half their duration;
/// bounds depend on transcript neighbours, not whether they are kept in the edit.
/// Contradictory shared constraints and non-positive snapped ranges are errors.
pub(super) fn snapped_cue_bounds(
    cues: &[TranscriptCue],
    idx: usize,
    padding: f64,
    envelope: &AudioEnvelope,
) -> Result<SourceRange> {
    let cue = &cues[idx];
    let cue_start = cue.start.as_secs_f64();
    let cue_end = cue.end.as_secs_f64();

    let start = if idx == 0 {
        let target = (cue_start - padding).max(0.0);
        envelope.snapped_boundary_point(target, 0.0, cue_start)?
    } else {
        match boundary_between(cues, idx - 1, padding, envelope)? {
            Boundary::Shared(cut) | Boundary::Split { start: cut, .. } => cut,
        }
    };

    let end = if idx + 1 == cues.len() {
        let target = cue_end + padding;
        envelope.snapped_boundary_point(target, cue_end, f64::INFINITY)?
    } else {
        match boundary_between(cues, idx, padding, envelope)? {
            Boundary::Shared(cut) | Boundary::Split { end: cut, .. } => cut,
        }
    };

    ensure!(
        end > start,
        "Non-positive snapped range for source `{}` cue #{}: [{start}, {end}]",
        cue.source_id,
        cue.cue_id
    );
    Ok(SourceRange::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::support::transcript::WordTiming;
    use std::f64::consts::TAU;
    use std::io::Cursor;
    use std::time::Duration;

    const RATE: u32 = 16_000;

    /// Build s16le PCM from `(duration_seconds, amplitude)` pieces. Non-zero
    /// amplitude is a 200 Hz tone; zero is digital silence.
    fn pcm(pieces: &[(f64, f64)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut n = 0u64;
        for &(seconds, amplitude) in pieces {
            let samples = (seconds * f64::from(RATE)).round() as u64;
            for _ in 0..samples {
                let value = amplitude * (TAU * 200.0 * n as f64 / f64::from(RATE)).sin() * 32767.0;
                out.extend_from_slice(&(value as i16).to_le_bytes());
                n += 1;
            }
        }
        out
    }

    fn envelope(pieces: &[(f64, f64)]) -> AudioEnvelope {
        AudioEnvelope::from_s16le_reader(Cursor::new(pcm(pieces)), RATE).unwrap()
    }

    fn word(text: &str, start: f64, end: f64) -> WordTiming {
        WordTiming {
            word: text.to_string(),
            start: Duration::from_secs_f64(start),
            end: Duration::from_secs_f64(end),
        }
    }

    fn cue(id: usize, words: Vec<WordTiming>) -> TranscriptCue {
        TranscriptCue {
            cue_id: id,
            start: words.first().unwrap().start,
            end: words.last().unwrap().end,
            text: words
                .iter()
                .map(|w| w.word.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            words,
            source_id: "a".to_string(),
        }
    }

    #[test]
    fn envelope_handles_odd_chunking_and_reports_duration() {
        let bytes = pcm(&[(1.0, 0.5)]);
        // A reader that returns one byte at a time exercises the carry byte.
        struct OneByte(Cursor<Vec<u8>>);
        impl Read for OneByte {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(&mut buf[..1])
            }
        }
        let chunked =
            AudioEnvelope::from_s16le_reader(OneByte(Cursor::new(bytes.clone())), RATE).unwrap();
        let whole = AudioEnvelope::from_s16le_reader(Cursor::new(bytes), RATE).unwrap();
        assert!((whole.duration() - 1.0).abs() < 1e-9);
        assert_eq!(chunked.db, whole.db);
    }

    #[test]
    fn cut_moves_out_of_a_word_tail_into_the_following_silence() {
        // Speech until 1.00 s, silence 1.00-1.40 s. The stamp (0.95) is early:
        // speech really continues to 1.00. Target = stamp + 80 ms = 1.03.
        let env = envelope(&[(1.0, 0.5), (0.4, 0.0), (0.6, 0.5)]);
        let cut = env.quietest_point(0.95 + 0.08, 0.95, 0.95 + 0.08 + SNAP_WINDOW_SECONDS);
        assert!(cut >= 1.0, "cut {cut} is still inside the word");
        assert!(cut <= 1.4);
    }

    #[test]
    fn silent_plateau_keeps_the_target_exactly() {
        let env = envelope(&[(0.5, 0.5), (1.0, 0.0), (0.5, 0.5)]);
        let target = 1.0123;
        assert_eq!(env.quietest_point(target, 0.7, 1.3), target);
    }

    #[test]
    fn result_never_leaves_the_requested_bounds() {
        let env = envelope(&[(0.5, 0.5), (0.3, 0.0), (0.5, 0.5)]);
        for (target, lo, hi) in [(0.2, 0.1, 0.3), (0.55, 0.52, 0.58), (1.2, 1.0, 1.1)] {
            let cut = env.quietest_point(target, lo, hi);
            assert!(
                (lo..=hi).contains(&cut),
                "{cut} outside [{lo}, {hi}] for target {target}"
            );
        }
        // Empty or out-of-recording bounds fall back to the target.
        assert_eq!(env.quietest_point(0.4, 0.6, 0.5), 0.4);
        assert_eq!(env.quietest_point(9.0, 8.0, 10.0), 9.0);
    }

    #[test]
    fn abutting_words_are_cut_at_the_dip_not_at_the_stamp() {
        // Loud speech with a 20 ms dip at 0.52-0.54 s. The ASR stamp says the
        // words meet at 0.50 s (20+ ms early).
        let env = envelope(&[(0.52, 0.5), (0.02, 0.0), (0.46, 0.5)]);
        let cues = vec![
            cue(0, vec![word("hello", 0.30, 0.50)]),
            cue(1, vec![word("world", 0.50, 0.90)]),
        ];

        let first = snapped_cue_bounds(&cues, 0, 0.08, &env).unwrap();
        let second = snapped_cue_bounds(&cues, 1, 0.08, &env).unwrap();

        assert_eq!(
            first.end, second.start,
            "kept neighbours must stay contiguous"
        );
        assert!(
            (0.52..=0.54).contains(&first.end),
            "cut {} missed the dip",
            first.end
        );
    }

    #[test]
    fn malformed_overlapping_cues_reject_contradictory_word_half_bounds() {
        let cues = vec![
            cue(7, vec![word("left", 0.8, 1.0)]),
            cue(9, vec![word("right", 0.5, 0.6)]),
        ];
        // Invalid timing must fail even when the envelope is unavailable.
        for env in [AudioEnvelope { db: vec![] }, envelope(&[(2.0, 0.5)])] {
            for idx in 0..cues.len() {
                let error = snapped_cue_bounds(&cues, idx, 0.08, &env).unwrap_err();
                let message = error.to_string();
                assert!(message.contains("Contradictory shared word half bounds"));
                assert!(message.contains("source `a`"));
                assert!(message.contains("cue #7"));
                assert!(message.contains("cue #9"));
            }
        }
    }

    #[test]
    fn shared_bounds_outside_the_target_window_are_rejected() {
        let env = envelope(&[(3.0, 0.5)]);
        let cues = vec![
            cue(7, vec![word("left", 0.0, 1.8), word("tail", 1.8, 2.0)]),
            cue(9, vec![word("right", 1.0, 3.0)]),
        ];
        // The raw bounds [1.9, 2.0] are valid, but cannot intersect the
        // midpoint target's 1.44-1.56 s window. Do not guess a cut at 1.5 s.
        for idx in 0..cues.len() {
            let error = snapped_cue_bounds(&cues, idx, 0.08, &env).unwrap_err();
            let message = format!("{error:#}");
            assert!(message.contains("Cannot snap shared boundary"));
            assert!(message.contains("source `a`"));
            assert!(message.contains("cue #7"));
            assert!(message.contains("cue #9"));
            assert!(message.contains("No valid cut within"));
        }
    }

    #[test]
    fn non_positive_snapped_ranges_are_rejected() {
        let env = AudioEnvelope { db: vec![] };
        for (start, end) in [(0.5, 0.5), (1.0, 0.5)] {
            let cues = vec![cue(7, vec![word("invalid", start, end)])];
            let error = snapped_cue_bounds(&cues, 0, 0.0, &env).unwrap_err();
            let message = error.to_string();
            assert!(message.contains("Non-positive snapped range"));
            assert!(message.contains("source `a`"));
            assert!(message.contains("cue #7"));
        }
    }

    #[test]
    fn shared_cut_cannot_swallow_a_short_word() {
        // The next cue opens with a 40 ms word. A deep dip sits just beyond the
        // half-word limit (0.52 s); without the bound the cut would chase it and
        // remove most of that word.
        let env = envelope(&[(0.53, 0.5), (0.02, 0.0), (0.45, 0.5)]);
        let cues = vec![
            cue(0, vec![word("a", 0.20, 0.50)]),
            cue(1, vec![word("b", 0.50, 0.54), word("c", 0.54, 0.90)]),
        ];
        let cut = snapped_cue_bounds(&cues, 0, 0.08, &env).unwrap().end;
        // May enter the 40 ms word by at most half of it.
        assert!(cut <= 0.52 + 1e-9, "cut {cut} bit into the next word");
    }

    #[test]
    fn separated_cues_snap_inside_their_own_padding_and_never_past_the_midpoint() {
        // Words end at 1.0 and resume at 2.0; silence between them.
        let env = envelope(&[(1.0, 0.5), (1.0, 0.0), (1.0, 0.5)]);
        let cues = vec![
            cue(0, vec![word("one", 0.2, 1.0)]),
            cue(1, vec![word("two", 2.0, 2.8)]),
        ];
        let first = snapped_cue_bounds(&cues, 0, 0.08, &env).unwrap();
        let second = snapped_cue_bounds(&cues, 1, 0.08, &env).unwrap();

        assert!(first.end >= 1.0 && first.end <= 1.5, "end {}", first.end);
        assert!(
            second.start <= 2.0 && second.start >= 1.5,
            "start {}",
            second.start
        );
        // Silent plateau: identical to the classic 80 ms pad.
        assert!((first.end - 1.08).abs() < 1e-9);
        assert!((second.start - 1.92).abs() < 1e-9);
    }

    #[test]
    fn split_bounds_ignore_deep_minima_beyond_the_snap_window() {
        let mut env = AudioEnvelope {
            db: vec![-10.0; 600],
        };
        // Targets are 1.20 and 1.80. The deepest minima are inside the old
        // word/midpoint bounds but more than 60 ms from those targets.
        env.db[206] = -100.0; // 1.0325 s
        env.db[393] = -100.0; // 1.9675 s
        env.db[234] = -30.0; // 1.1725 s
        env.db[365] = -30.0; // 1.8275 s
        let cues = vec![
            cue(0, vec![word("one", 0.2, 1.0)]),
            cue(1, vec![word("two", 2.0, 2.8)]),
        ];
        let first = snapped_cue_bounds(&cues, 0, 0.2, &env).unwrap();
        let second = snapped_cue_bounds(&cues, 1, 0.2, &env).unwrap();

        assert!((first.end - 1.2).abs() <= SNAP_WINDOW_SECONDS);
        assert!((second.start - 1.8).abs() <= SNAP_WINDOW_SECONDS);
        assert!((first.end - frame_centre(234)).abs() <= HOP_SECONDS / 2.0 + 1e-9);
        assert!((second.start - frame_centre(365)).abs() <= HOP_SECONDS / 2.0 + 1e-9);
        // The generic search must still honour wider caller-supplied bounds.
        assert!(env.quietest_point(1.2, 1.0, 1.5) < 1.2 - SNAP_WINDOW_SECONDS);
        assert!(env.quietest_point(1.8, 1.5, 2.0) > 1.8 + SNAP_WINDOW_SECONDS);
    }

    #[test]
    fn outer_bounds_ignore_deep_minima_beyond_the_snap_window() {
        let mut env = AudioEnvelope {
            db: vec![-10.0; 600],
        };
        // Targets are 0.80 and 2.20. Deep minima toward the word stamps
        // previously pulled both outer cuts more than 60 ms from the target.
        env.db[193] = -100.0; // 0.9675 s
        env.db[406] = -100.0; // 2.0325 s
        env.db[165] = -30.0; // 0.8275 s
        env.db[434] = -30.0; // 2.1725 s
        let cues = vec![cue(0, vec![word("one", 1.0, 2.0)])];
        let bounds = snapped_cue_bounds(&cues, 0, 0.2, &env).unwrap();

        assert!((bounds.start - 0.8).abs() <= SNAP_WINDOW_SECONDS);
        assert!((bounds.end - 2.2).abs() <= SNAP_WINDOW_SECONDS);
        assert!((bounds.start - frame_centre(165)).abs() <= HOP_SECONDS / 2.0 + 1e-9);
        assert!((bounds.end - frame_centre(434)).abs() <= HOP_SECONDS / 2.0 + 1e-9);
    }

    #[test]
    fn kept_cue_start_stays_clear_of_the_previous_words_real_tail() {
        // Bounds depend only on transcript neighbours, never on what the
        // document keeps. 'hello' is stamped to end at 0.50 but really sounds
        // until 0.52; if that line is deleted, none of it may leak into 'world'.
        let env = envelope(&[(0.52, 0.5), (0.02, 0.0), (0.46, 0.5)]);
        let cues = vec![
            cue(0, vec![word("hello", 0.30, 0.50)]),
            cue(1, vec![word("world", 0.50, 0.90)]),
        ];
        let kept = snapped_cue_bounds(&cues, 1, 0.08, &env).unwrap();
        assert!(
            kept.start >= 0.52 - 1e-9,
            "start {} reaches into 'hello'",
            kept.start
        );
    }
}
