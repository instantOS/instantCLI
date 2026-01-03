//! Subtitle timing remapper.
//!
//! Remaps transcript cue timings from source video time to final timeline time.
//! This is necessary because segments can be reordered in the markdown document,
//! and pauses can be inserted between segments.

use std::time::Duration;

use crate::video::render_timeline::{SegmentData, Timeline};
use crate::video::transcript::TranscriptCue;

/// A remapped word with timing adjusted to the final timeline.
#[derive(Debug, Clone)]
pub struct RemappedWord {
    pub word: String,
    pub start: Duration,
    pub end: Duration,
}

/// A subtitle with timing remapped to the final timeline.
#[derive(Debug, Clone)]
pub struct RemappedSubtitle {
    /// Start time in the final rendered video
    pub start: Duration,
    /// End time in the final rendered video
    pub end: Duration,
    /// Subtitle text (full phrase)
    pub text: String,
    /// Individual word timings for karaoke highlighting
    pub words: Vec<RemappedWord>,
}

/// Minimum subtitle display duration in seconds.
const MIN_SUBTITLE_DURATION_SECS: f64 = 0.5;

/// Remap transcript cues from source video time to final timeline time.
///
/// For each video segment in the timeline, finds transcript cues that overlap
/// with the segment's source time range and remaps them to the final timeline.
/// Word-level timings are preserved for karaoke-style highlighting.
///
/// # Arguments
/// * `timeline` - The render timeline with segments in final order
/// * `cues` - Transcript cues with source video timing
///
/// # Returns
/// A vector of subtitles with timing adjusted to the final timeline.
pub fn remap_subtitles_to_timeline(
    timeline: &Timeline,
    cues: &[TranscriptCue],
) -> Vec<RemappedSubtitle> {
    let mut subtitles = Vec::new();

    for segment in &timeline.segments {
        let SegmentData::VideoSubset {
            start_time: source_start,
            mute_audio,
            ..
        } = &segment.data
        else {
            continue;
        };

        // Skip muted segments (title cards, etc.) - no subtitles
        if *mute_audio {
            continue;
        }

        let source_end = source_start + segment.duration;
        // Time offset to convert source time to final timeline time
        let time_offset = segment.start_time - source_start;

        // Find cues that overlap with this segment's source time range
        for cue in cues {
            let cue_start = cue.start.as_secs_f64();
            let cue_end = cue.end.as_secs_f64();

            // Check for overlap with source range
            if cue_end <= *source_start || cue_start >= source_end {
                continue; // No overlap
            }

            // Calculate the portion of the cue that falls within this segment
            let overlap_start = cue_start.max(*source_start);
            let overlap_end = cue_end.min(source_end);

            // Remap cue timing to final timeline
            let final_start = overlap_start + time_offset;
            let final_end = overlap_end + time_offset;

            // Remap word timings, filtering to only words within this segment
            let remapped_words: Vec<RemappedWord> = cue
                .words
                .iter()
                .filter_map(|word| {
                    let word_start = word.start.as_secs_f64();
                    let word_end = word.end.as_secs_f64();

                    // Check if word overlaps with segment's source range
                    if word_end <= *source_start || word_start >= source_end {
                        return None;
                    }

                    // Clamp word timing to segment boundaries and remap
                    let clamped_start = word_start.max(*source_start);
                    let clamped_end = word_end.min(source_end);

                    Some(RemappedWord {
                        word: word.word.clone(),
                        start: Duration::from_secs_f64(clamped_start + time_offset),
                        end: Duration::from_secs_f64(clamped_end + time_offset),
                    })
                })
                .collect();

            // Ensure minimum duration for readability
            let duration = final_end - final_start;
            let adjusted_end = if duration < MIN_SUBTITLE_DURATION_SECS {
                (final_start + MIN_SUBTITLE_DURATION_SECS)
                    .min(segment.start_time + segment.duration)
            } else {
                final_end
            };

            // Build text from remapped words if available, otherwise use cue text
            let text = if remapped_words.is_empty() {
                cue.text.clone()
            } else {
                remapped_words
                    .iter()
                    .map(|w| w.word.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            };

            subtitles.push(RemappedSubtitle {
                start: Duration::from_secs_f64(final_start),
                end: Duration::from_secs_f64(adjusted_end),
                text,
                words: remapped_words,
            });
        }
    }

    // Sort by start time (segments might not be in chronological source order)
    subtitles.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    // Merge overlapping subtitles from the same cue that got split
    merge_overlapping_subtitles(subtitles)
}

/// Merge subtitles that overlap and have the same text.
fn merge_overlapping_subtitles(subtitles: Vec<RemappedSubtitle>) -> Vec<RemappedSubtitle> {
    if subtitles.is_empty() {
        return subtitles;
    }

    let mut iter = subtitles.into_iter();
    let mut current = iter.next().unwrap();
    let mut result = Vec::new();

    for next in iter {
        // If same text and truly overlapping (not just adjacent), merge
        // We don't merge adjacent subtitles because they may come from
        // non-contiguous source regions (e.g., when source content is cut)
        if current.text == next.text && next.start < current.end {
            current.end = current.end.max(next.end);
            // Merge words, avoiding duplicates
            for word in next.words {
                if !current
                    .words
                    .iter()
                    .any(|w| w.word == word.word && w.start == word.start && w.end == word.end)
                {
                    current.words.push(word);
                }
            }
            // Re-sort words by start time after merge
            current
                .words
                .sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        } else {
            result.push(current);
            current = next;
        }
    }
    result.push(current);

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::render_timeline::{Segment, SegmentData, Timeline};
    use std::path::PathBuf;

    fn make_cue(start_ms: u64, end_ms: u64, text: &str) -> TranscriptCue {
        TranscriptCue {
            start: Duration::from_millis(start_ms),
            end: Duration::from_millis(end_ms),
            text: text.to_string(),
            words: vec![], // No word-level timing for basic tests
        }
    }

    fn make_video_segment(
        start_time: f64,
        duration: f64,
        source_start: f64,
        mute: bool,
    ) -> Segment {
        Segment {
            start_time,
            duration,
            data: SegmentData::VideoSubset {
                start_time: source_start,
                source_video: PathBuf::from("test.mp4"),
                transform: None,
                mute_audio: mute,
            },
        }
    }

    #[test]
    fn test_simple_remap() {
        // Source video: cue at 0-2s saying "Hello"
        // Timeline: single segment playing source 0-2s at final 0-2s
        let cues = vec![make_cue(0, 2000, "Hello")];

        let mut timeline = Timeline::new();
        timeline.add_segment(make_video_segment(0.0, 2.0, 0.0, false));

        let subtitles = remap_subtitles_to_timeline(&timeline, &cues);

        assert_eq!(subtitles.len(), 1);
        assert_eq!(subtitles[0].text, "Hello");
        assert_eq!(subtitles[0].start.as_secs_f64(), 0.0);
        assert_eq!(subtitles[0].end.as_secs_f64(), 2.0);
    }

    #[test]
    fn test_reordered_segments() {
        // Source: cue1 at 0-2s, cue2 at 5-7s
        // Timeline: play source 5-7s first (at 0-2s), then source 0-2s (at 2-4s)
        let cues = vec![
            make_cue(0, 2000, "First in source"),
            make_cue(5000, 7000, "Second in source"),
        ];

        let mut timeline = Timeline::new();
        // Play source 5-7s at final 0-2s
        timeline.add_segment(make_video_segment(0.0, 2.0, 5.0, false));
        // Play source 0-2s at final 2-4s
        timeline.add_segment(make_video_segment(2.0, 2.0, 0.0, false));

        let subtitles = remap_subtitles_to_timeline(&timeline, &cues);

        assert_eq!(subtitles.len(), 2);
        // "Second in source" should now appear first (at 0-2s)
        assert_eq!(subtitles[0].text, "Second in source");
        assert_eq!(subtitles[0].start.as_secs_f64(), 0.0);
        assert_eq!(subtitles[0].end.as_secs_f64(), 2.0);
        // "First in source" should appear second (at 2-4s)
        assert_eq!(subtitles[1].text, "First in source");
        assert_eq!(subtitles[1].start.as_secs_f64(), 2.0);
        assert_eq!(subtitles[1].end.as_secs_f64(), 4.0);
    }

    #[test]
    fn test_gap_between_segments() {
        // Source: cue at 0-2s
        // Timeline: play source 0-2s at final 5-7s (with 5s gap before)
        let cues = vec![make_cue(0, 2000, "Delayed")];

        let mut timeline = Timeline::new();
        timeline.add_segment(make_video_segment(5.0, 2.0, 0.0, false));

        let subtitles = remap_subtitles_to_timeline(&timeline, &cues);

        assert_eq!(subtitles.len(), 1);
        assert_eq!(subtitles[0].text, "Delayed");
        assert_eq!(subtitles[0].start.as_secs_f64(), 5.0);
        assert_eq!(subtitles[0].end.as_secs_f64(), 7.0);
    }

    #[test]
    fn test_muted_segment_no_subtitles() {
        // Source: cue at 0-2s
        // Timeline: muted segment (title card)
        let cues = vec![make_cue(0, 2000, "Should not appear")];

        let mut timeline = Timeline::new();
        timeline.add_segment(make_video_segment(0.0, 2.0, 0.0, true)); // muted

        let subtitles = remap_subtitles_to_timeline(&timeline, &cues);

        assert!(subtitles.is_empty());
    }

    #[test]
    fn test_cue_spanning_segment_boundary() {
        // Source: cue at 1-4s (spans across what will be two segments)
        // Timeline: segment1 plays source 0-2s, segment2 plays source 3-5s
        // The cue overlaps both: 1-2s in segment1, 3-4s in segment2
        let cues = vec![make_cue(1000, 4000, "Spanning")];

        let mut timeline = Timeline::new();
        timeline.add_segment(make_video_segment(0.0, 2.0, 0.0, false)); // source 0-2
        timeline.add_segment(make_video_segment(2.0, 2.0, 3.0, false)); // source 3-5

        let subtitles = remap_subtitles_to_timeline(&timeline, &cues);

        // Should get two subtitle entries (one for each segment portion)
        assert_eq!(subtitles.len(), 2);
        // First portion: cue 1-2s remapped to final 1-2s
        assert_eq!(subtitles[0].text, "Spanning");
        assert!((subtitles[0].start.as_secs_f64() - 1.0).abs() < 0.01);
        assert!((subtitles[0].end.as_secs_f64() - 2.0).abs() < 0.01);
        // Second portion: cue 3-4s remapped to final 2-3s (offset by segment.start_time - source_start)
        assert_eq!(subtitles[1].text, "Spanning");
        assert!((subtitles[1].start.as_secs_f64() - 2.0).abs() < 0.01);
        assert!((subtitles[1].end.as_secs_f64() - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_minimum_duration() {
        // Source: very short cue (100ms)
        let cues = vec![make_cue(0, 100, "Quick")];

        let mut timeline = Timeline::new();
        timeline.add_segment(make_video_segment(0.0, 2.0, 0.0, false));

        let subtitles = remap_subtitles_to_timeline(&timeline, &cues);

        assert_eq!(subtitles.len(), 1);
        // Should be extended to minimum duration
        assert!(subtitles[0].end.as_secs_f64() >= MIN_SUBTITLE_DURATION_SECS);
    }
}
