use crate::video::support::transcript::TranscriptCue;
use crate::video::support::utils::duration_to_tenths;
use chrono::Utc;
use std::time::Duration;

use super::VideoMetadata;

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
        lines.push(format!(
            "`{}@{}-{}` {}",
            source_id,
            format_timestamp(cue.start),
            format_timestamp(cue.end),
            cue.text.trim()
        ));

        previous_end = cue.end;
    }

    let front_matter = render_frontmatter(metadata);

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

pub fn render_frontmatter(metadata: &VideoMetadata) -> String {
    let timestamp = Utc::now().to_rfc3339();
    let default_source = yaml_quote(metadata.default_source.as_deref().unwrap_or("a"));
    let mut source_lines = Vec::new();
    for source in &metadata.sources {
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
    use std::path::{Path, PathBuf};

    fn cue(start: u64, end: u64, text: &str) -> TranscriptCue {
        TranscriptCue {
            start: Duration::from_millis(start),
            end: Duration::from_millis(end),
            text: text.to_string(),
            words: vec![],
            source_id: "a".to_string(),
        }
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
