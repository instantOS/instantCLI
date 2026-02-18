use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};

use crate::ui::prelude::Level;
use crate::video::audio::{PreprocessorType, create_preprocessor};
use crate::video::config::VideoConfig;
use crate::video::document::VideoDocument;
use crate::video::document::{VideoMetadata, VideoSource};
use crate::video::render::logging::log_event;
use crate::video::support::utils::canonicalize_existing;

pub(super) fn resolve_source_path(path: &Path, project_dir: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(project_dir.join(path))
    }
}

pub(super) fn find_default_source<'a>(
    metadata: &VideoMetadata,
    sources: &'a [VideoSource],
) -> Result<&'a VideoSource> {
    let default_id = metadata
        .default_source
        .as_ref()
        .or_else(|| sources.first().map(|source| &source.id))
        .ok_or_else(|| anyhow!("No video sources available"))?;
    sources
        .iter()
        .find(|source| &source.id == default_id)
        .ok_or_else(|| anyhow!("Default source `{}` not found", default_id))
}

pub(super) fn validate_timeline_sources(
    document: &VideoDocument,
    sources: &[VideoSource],
    cues: &[crate::video::support::transcript::TranscriptCue],
) -> Result<()> {
    let mut referenced_sources = HashSet::new();
    for block in &document.blocks {
        if let crate::video::document::DocumentBlock::Segment(segment) = block {
            referenced_sources.insert(segment.source_id.clone());
        }
    }

    let mut available_sources = HashSet::new();
    for source in sources {
        available_sources.insert(source.id.clone());
    }

    for source_id in &referenced_sources {
        if !available_sources.contains(source_id) {
            bail!("Timeline references unknown source `{}`", source_id);
        }
    }

    let mut cue_sources = HashSet::new();
    for cue in cues {
        cue_sources.insert(cue.source_id.clone());
    }

    for source_id in &referenced_sources {
        if !cue_sources.contains(source_id) {
            bail!(
                "No transcript cues loaded for source `{}`; check front matter transcripts",
                source_id
            );
        }
    }

    Ok(())
}

pub(crate) async fn resolve_video_sources(
    metadata: &VideoMetadata,
    project_dir: &Path,
    config: &VideoConfig,
) -> Result<Vec<VideoSource>> {
    let mut resolved = Vec::new();
    for source in &metadata.sources {
        let resolved_source = resolve_source_path(&source.source, project_dir)?;
        let resolved_transcript = resolve_source_path(&source.transcript, project_dir)?;
        let resolved_audio = resolve_audio_path(&resolved_source, config).await?;
        let canonical = canonicalize_existing(&resolved_source)?;
        log_event(
            Level::Info,
            "video.render.video",
            format!("Using source {} video {}", source.id, canonical.display()),
        );
        resolved.push(VideoSource {
            id: source.id.clone(),
            name: source.name.clone(),
            source: resolved_source,
            transcript: resolved_transcript,
            audio: resolved_audio,
            hash: source.hash.clone(),
        });
    }

    Ok(resolved)
}

async fn resolve_audio_path(video_path: &Path, config: &VideoConfig) -> Result<PathBuf> {
    if matches!(config.preprocessor, PreprocessorType::None) {
        log_event(
            Level::Info,
            "video.render.audio",
            "Preprocessing disabled. Using original video audio.",
        );
        return Ok(video_path.to_path_buf());
    }

    let preprocessor = create_preprocessor(&config.preprocessor, config);

    if !preprocessor.is_available() {
        log_event(
            Level::Warn,
            "video.render.audio.preprocess",
            format!(
                "Preprocessor '{}' not available. Using original video audio.",
                preprocessor.name()
            ),
        );
        return Ok(video_path.to_path_buf());
    }

    let result = preprocessor.process(video_path, false).await?;

    log_event(
        Level::Info,
        "video.render.audio.preprocess",
        format!("Using preprocessed audio: {}", result.output_path.display()),
    );

    Ok(result.output_path)
}

/// Build a mapping from video source ID to its resolved audio file path.
///
/// Each video source can have a separate audio track (e.g. preprocessed audio).
/// This map lets the ffmpeg compiler look up the audio file for any source by ID.
pub(super) fn build_audio_source_map(sources: &[VideoSource]) -> HashMap<String, PathBuf> {
    sources
        .iter()
        .map(|s| (s.id.clone(), s.audio.clone()))
        .collect()
}
