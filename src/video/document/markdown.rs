use crate::video::support::transcript::TranscriptCue;
use crate::video::support::utils::duration_to_tenths;
use chrono::Utc;
use std::time::Duration;

use super::VideoMetadata;

/// Formats a transcript cue as a video-markdown line. The compact `#N`
/// component is stable identity; rounded timestamps remain human-readable.
pub fn format_cue_line(source_id: &str, cue: &TranscriptCue) -> String {
    format!(
        "`{}#{}@{}-{}` {}",
        source_id,
        cue.cue_id,
        format_timestamp(cue.start),
        format_timestamp(cue.end),
        cue.text.trim()
    )
}

pub fn build_markdown(cues: &[TranscriptCue], metadata: &VideoMetadata) -> String {
    let default_source = metadata.default_source.as_deref().unwrap_or("a");
    let mut lines = Vec::with_capacity(cues.len() * 2);
    let mut previous_end = Duration::from_secs(0);

    for cue in cues {
        let source_id = if cue.source_id.trim().is_empty() {
            default_source
        } else {
            cue.source_id.as_str()
        };
        if cue.start > previous_end {
            let silence_gap = cue.start - previous_end;
            if silence_gap >= Duration::from_secs(1) {
                insert_silence_lines(&mut lines, previous_end, cue.start, source_id);
            }
        }
        lines.push(format_cue_line(source_id, cue));

        previous_end = cue.end;
    }

    let front_matter = metadata.render_frontmatter();

    if lines.is_empty() {
        front_matter
    } else {
        format!("{front_matter}\n{}\n", lines.join("\n"))
    }
}

fn insert_silence_lines(
    lines: &mut Vec<String>,
    mut start: Duration,
    end: Duration,
    source_id: &str,
) {
    let max_chunk = Duration::from_secs(5);
    while start < end {
        let chunk_end = std::cmp::min(end, start + max_chunk);
        lines.push(format!(
            "`{}@{}-{}` SILENCE",
            source_id,
            format_timestamp(start),
            format_timestamp(chunk_end)
        ));
        start = chunk_end;
    }
}

impl VideoMetadata {
    pub fn render_frontmatter(&self) -> String {
        let timestamp = Utc::now().to_rfc3339();
        let default_source = yaml_quote(self.default_source.as_deref().unwrap_or("a"));
        let mut source_lines = Vec::new();
        for source in &self.sources {
            let source_id = yaml_quote(&source.id);
            let video_source = yaml_quote(&source.source.to_string_lossy());
            let transcript_source = yaml_quote(&source.transcript.to_string_lossy());
            let video_hash = yaml_quote(source.hash.as_deref().unwrap_or(""));
            source_lines.push(format!(
                "- id: {source_id}\n  hash: {video_hash}\n  name: {name}\n  source: {video_source}\n  transcript: {transcript_source}",
                name = yaml_quote(source.name.as_deref().unwrap_or("")),
            ));
        }
        if source_lines.is_empty() {
            return format!(
                "---\ndefault_source: {default_source}\nsources: []\ngenerated_at: '{timestamp}'\n---"
            );
        }

        let sources_block = source_lines.join("\n");
        format!(
            "---\ndefault_source: {default_source}\nsources:\n{sources}\ngenerated_at: '{timestamp}'\n---",
            sources = sources_block,
        )
    }
}

fn yaml_quote(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

pub fn format_timestamp(duration: Duration) -> String {
    let total_tenths = duration_to_tenths(duration);
    let total_secs = total_tenths / 10;
    let tenths = total_tenths % 10;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours == 0 {
        format!("{minutes:02}:{seconds:02}.{tenths}")
    } else {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{tenths}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::VideoSource;
    use std::path::PathBuf;

    fn cue(start: u64, end: u64, text: &str) -> TranscriptCue {
        TranscriptCue {
            cue_id: 0,
            start: Duration::from_millis(start),
            end: Duration::from_millis(end),
            text: text.to_string(),
            words: vec![],
            source_id: "a".to_string(),
        }
    }

    #[test]
    fn generated_cue_references_round_trip() -> anyhow::Result<()> {
        use crate::video::document::{DocumentBlock, SegmentKind, parse_video_document};

        // Exercise both single-source and multi-source metadata validation.
        for source_ids in [vec!["a"], vec!["a", "b"]] {
            let metadata = VideoMetadata {
                sources: source_ids
                    .iter()
                    .map(|id| VideoSource {
                        id: (*id).to_string(),
                        name: None,
                        source: PathBuf::from(format!("{id}.mp4")),
                        transcript: PathBuf::from(format!("{id}.json")),
                        audio: PathBuf::new(),
                        hash: None,
                    })
                    .collect(),
                default_source: Some("a".to_string()),
            };
            let source_id = source_ids.last().copied().unwrap_or("a");
            let mut cue = cue(10_000, 10_300, "hi");
            cue.cue_id = 2;
            cue.source_id = source_id.to_string();
            let markdown = build_markdown(&[cue], &metadata);
            let document = parse_video_document(&markdown, std::path::Path::new("test.md"))?;
            let dialogue: Vec<_> = document
                .blocks
                .iter()
                .filter_map(|block| match block {
                    DocumentBlock::Segment(segment) if segment.kind == SegmentKind::Dialogue => {
                        Some(segment)
                    }
                    _ => None,
                })
                .collect();

            assert_eq!(dialogue.len(), 1);
            assert_eq!(dialogue[0].source_id, source_id);
            assert_eq!(dialogue[0].cue_id, Some(2));
            assert_eq!(dialogue[0].text, "hi");
            assert_eq!(dialogue[0].range.start, Duration::from_millis(10_000));
            assert_eq!(dialogue[0].range.end, Duration::from_millis(10_300));
        }
        Ok(())
    }

    #[test]
    fn inserts_silence_chunks() {
        let cues = vec![cue(3000, 4000, "Hello"), cue(11000, 12000, "World")];
        let metadata = VideoMetadata {
            sources: vec![VideoSource {
                id: "a".to_string(),
                name: Some("clip.mp4".to_string()),
                source: PathBuf::from("/video/clip.mp4"),
                transcript: PathBuf::from("/tmp/clip.srt"),
                audio: PathBuf::new(),
                hash: Some("hash".to_string()),
            }],
            default_source: Some("a".to_string()),
        };

        let output = build_markdown(&cues, &metadata);
        assert!(output.contains("`a@00:04.0-00:09.0` SILENCE"));
        assert!(output.contains("`a@00:09.0-00:11.0` SILENCE"));
    }
}
