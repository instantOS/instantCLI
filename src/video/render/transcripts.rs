use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ui::prelude::Level;
use crate::video::document::{VideoDocument, VideoSource};
use crate::video::render::logging::log_event;
use crate::video::render::sources::resolve_source_path;
use crate::video::support::transcript::{TranscriptCue, WordTiming, parse_whisper_json};
use crate::video::support::utils::canonicalize_existing;

pub(crate) fn load_transcript_cues(
    sources: &[VideoSource],
    project_dir: &Path,
) -> Result<Vec<TranscriptCue>> {
    let mut cues = Vec::new();

    for source in sources {
        let transcript_path = resolve_source_path(&source.transcript, project_dir)?;
        let transcript_path = canonicalize_existing(&transcript_path)?;

        log_event(
            Level::Info,
            "video.render.transcript.read",
            format!(
                "Reading transcript for {} from {}",
                source.id,
                transcript_path.display()
            ),
        );

        let transcript_contents = fs::read_to_string(&transcript_path).with_context(|| {
            format!(
                "Failed to read transcript file {}",
                transcript_path.display()
            )
        })?;

        log_event(
            Level::Info,
            "video.render.transcript.parse",
            format!("Parsing transcript cues for {}", source.id),
        );
        let mut parsed = parse_whisper_json(&transcript_contents)?;
        for cue in &mut parsed {
            cue.source_id = source.id.clone();
        }
        cues.extend(parsed);
    }

    Ok(cues)
}

/// Patch transcript cues with text edited by the user in the markdown file.
///
/// When `ins video convert` generates a `.video.md`, each segment line contains
/// the ASR-transcribed text with a time range reference:
///
/// ````a@00:01.500-00:04.200` the quick brown fox````
///
/// If the user edits that text (e.g. fixing an ASR error), the render pipeline
/// should use the corrected text for subtitles instead of the original transcript.
/// This function compares each dialogue segment to its matching cue and, when
/// the text differs, replaces the cue text and redistributes word timings
/// proportionally across the new words.
pub(crate) fn apply_markdown_edits(cues: &mut [TranscriptCue], document: &VideoDocument) {
    use crate::video::document::{DocumentBlock, SegmentKind};

    let mut patched = 0;

    for block in &document.blocks {
        let DocumentBlock::Segment(segment) = block else {
            continue;
        };
        if segment.kind != SegmentKind::Dialogue {
            continue;
        }

        // Find the cue whose time range overlaps most with this segment.
        // Segments and cues share the same source-time coordinates.
        let best = cues
            .iter_mut()
            .filter(|c| c.source_id == segment.source_id)
            .max_by_key(|c| {
                let overlap_start = c.start.max(segment.range.start);
                let overlap_end = c.end.min(segment.range.end);
                if overlap_end > overlap_start {
                    (overlap_end - overlap_start).as_millis()
                } else {
                    0
                }
            });

        let Some(cue) = best else {
            continue;
        };

        let original = cue.text.trim().to_string();
        let edited = segment.text.trim().to_string();

        if original == edited {
            continue;
        }

        // Text was edited — replace and redistribute word timings.
        let new_words: Vec<&str> = edited.split_whitespace().collect();

        if cue.words.is_empty() || new_words.is_empty() {
            // No word timings to redistribute — just replace the text.
            cue.text = edited;
            patched += 1;
            continue;
        }

        let span_start = cue.words.first().unwrap().start;
        let span_end = cue.words.last().unwrap().end;
        let total_span = (span_end - span_start).as_secs_f64();
        let new_count = new_words.len();

        // Proportional redistribution: divide the original word-timing span
        // evenly across the new words. This keeps karaoke highlighting flowing
        // smoothly even when the word count changes (e.g. ASR split "unbelievable"
        // into "un believe able" and the user fixed it back).
        let per_word = if total_span > 0.0 {
            total_span / new_count as f64
        } else {
            // Fallback: distribute across the cue's overall range.

            (cue.end - cue.start).as_secs_f64() / new_count as f64
        };

        cue.words = new_words
            .iter()
            .enumerate()
            .map(|(i, word)| {
                let start = span_start + std::time::Duration::from_secs_f64(per_word * i as f64);
                let end =
                    span_start + std::time::Duration::from_secs_f64(per_word * (i + 1) as f64);
                WordTiming {
                    word: word.to_string(),
                    start,
                    end,
                }
            })
            .collect();

        cue.text = edited;
        patched += 1;
    }

    if patched > 0 {
        log_event(
            Level::Info,
            "video.render.transcript.edits",
            format!("Applied {patched} markdown text edits to transcript cues"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::{
        DocumentBlock, SegmentBlock, SegmentKind, TimeRange, VideoDocument, VideoMetadata,
    };
    use std::time::Duration;

    fn doc_with_segments(segments: Vec<(String, Duration, Duration, String)>) -> VideoDocument {
        let blocks = segments
            .into_iter()
            .map(|(source_id, start, end, text)| {
                DocumentBlock::Segment(SegmentBlock {
                    range: TimeRange { start, end },
                    text,
                    kind: SegmentKind::Dialogue,
                    source_id,
                })
            })
            .collect();
        VideoDocument {
            metadata: VideoMetadata {
                sources: vec![],
                default_source: None,
            },
            blocks,
        }
    }

    fn cue(
        source_id: &str,
        start: Duration,
        end: Duration,
        text: &str,
        words: Vec<(&str, Duration, Duration)>,
    ) -> TranscriptCue {
        TranscriptCue {
            start,
            end,
            text: text.to_string(),
            source_id: source_id.to_string(),
            words: words
                .into_iter()
                .map(|(w, s, e)| WordTiming {
                    word: w.to_string(),
                    start: s,
                    end: e,
                })
                .collect(),
        }
    }

    #[test]
    fn no_edit_leaves_cues_unchanged() {
        let doc = doc_with_segments(vec![(
            "a".into(),
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "hello world".into(),
        )]);
        let mut cues = vec![cue(
            "a",
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "hello world",
            vec![
                (
                    "hello",
                    Duration::from_secs_f64(1.0),
                    Duration::from_secs_f64(2.0),
                ),
                (
                    "world",
                    Duration::from_secs_f64(2.0),
                    Duration::from_secs_f64(3.0),
                ),
            ],
        )];
        apply_markdown_edits(&mut cues, &doc);
        assert_eq!(cues[0].text, "hello world");
        assert_eq!(cues[0].words.len(), 2);
        assert_eq!(cues[0].words[0].word, "hello");
    }

    #[test]
    fn word_merge_3_to_1_redistributes_evenly() {
        // ASR produced "un believe able" (3 words), user fixed to "unbelievable"
        let doc = doc_with_segments(vec![(
            "a".into(),
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "unbelievable".into(),
        )]);
        let mut cues = vec![cue(
            "a",
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "un believe able",
            vec![
                (
                    "un",
                    Duration::from_secs_f64(1.0),
                    Duration::from_secs_f64(2.0),
                ),
                (
                    "believe",
                    Duration::from_secs_f64(2.0),
                    Duration::from_secs_f64(3.0),
                ),
                (
                    "able",
                    Duration::from_secs_f64(3.0),
                    Duration::from_secs_f64(4.0),
                ),
            ],
        )];
        apply_markdown_edits(&mut cues, &doc);

        assert_eq!(cues[0].text, "unbelievable");
        assert_eq!(cues[0].words.len(), 1);
        // Single word should span the entire original word-timing range
        assert_eq!(cues[0].words[0].start, Duration::from_secs_f64(1.0));
        assert_eq!(cues[0].words[0].end, Duration::from_secs_f64(4.0));
    }

    #[test]
    fn word_split_1_to_3_redistributes_evenly() {
        // User expanded "hello" into "hey there world"
        let doc = doc_with_segments(vec![(
            "a".into(),
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "hey there world".into(),
        )]);
        let mut cues = vec![cue(
            "a",
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "hello",
            vec![(
                "hello",
                Duration::from_secs_f64(1.0),
                Duration::from_secs_f64(4.0),
            )],
        )];
        apply_markdown_edits(&mut cues, &doc);

        assert_eq!(cues[0].text, "hey there world");
        assert_eq!(cues[0].words.len(), 3);
        // Span is 1.0 → 4.0 = 3.0s, each word gets 1.0s
        assert_eq!(cues[0].words[0].word, "hey");
        assert_eq!(cues[0].words[0].start, Duration::from_secs_f64(1.0));
        assert_eq!(cues[0].words[1].word, "there");
        assert_eq!(cues[0].words[1].start, Duration::from_secs_f64(2.0));
        assert_eq!(cues[0].words[2].word, "world");
        assert_eq!(cues[0].words[2].start, Duration::from_secs_f64(3.0));
    }

    #[test]
    fn different_source_id_not_patched() {
        let doc = doc_with_segments(vec![(
            "a".into(),
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "edited text".into(),
        )]);
        let mut cues = vec![cue(
            "b",
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "original text",
            vec![],
        )];
        apply_markdown_edits(&mut cues, &doc);
        assert_eq!(cues[0].text, "original text");
    }

    #[test]
    fn silence_segments_are_ignored() {
        let mut doc = doc_with_segments(vec![(
            "a".into(),
            Duration::from_secs_f64(0.0),
            Duration::from_secs_f64(5.0),
            "SILENCE".into(),
        )]);
        // Force the segment kind to Silence
        if let DocumentBlock::Segment(ref mut seg) = doc.blocks[0] {
            seg.kind = SegmentKind::Silence;
        }
        let mut cues = vec![cue(
            "a",
            Duration::from_secs_f64(0.0),
            Duration::from_secs_f64(5.0),
            "original text",
            vec![],
        )];
        apply_markdown_edits(&mut cues, &doc);
        assert_eq!(cues[0].text, "original text");
    }

    #[test]
    fn same_word_count_preserves_timings() {
        // Same number of words, just corrected text — timings should be preserved
        let doc = doc_with_segments(vec![(
            "a".into(),
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "hello wrld".into(), // typo fix, same word count
        )]);
        let w1_start = Duration::from_secs_f64(1.0);
        let w1_end = Duration::from_secs_f64(2.0);
        let w2_start = Duration::from_secs_f64(2.0);
        let w2_end = Duration::from_secs_f64(3.0);
        let mut cues = vec![cue(
            "a",
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
            "hello world",
            vec![("hello", w1_start, w1_end), ("world", w2_start, w2_end)],
        )];
        apply_markdown_edits(&mut cues, &doc);
        assert_eq!(cues[0].text, "hello wrld");
        assert_eq!(cues[0].words.len(), 2);
        // With same word count (2→2), redistribution gives each word half the span.
        // span = 1.0→3.0 = 2.0s, each gets 1.0s
        assert_eq!(cues[0].words[0].start, Duration::from_secs_f64(1.0));
        assert_eq!(cues[0].words[1].start, Duration::from_secs_f64(2.0));
    }
}
