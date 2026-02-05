use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ui::prelude::Level;
use crate::video::document::VideoSource;
use crate::video::render::logging::log_event;
use crate::video::render::sources::resolve_source_path;
use crate::video::support::transcript::{parse_whisper_json, TranscriptCue};
use crate::video::support::utils::canonicalize_existing;

pub(super) fn load_transcript_cues(
    sources: &[VideoSource],
    markdown_dir: &Path,
) -> Result<Vec<TranscriptCue>> {
    let mut cues = Vec::new();

    for source in sources {
        let transcript_path = resolve_source_path(&source.transcript, markdown_dir)?;
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
