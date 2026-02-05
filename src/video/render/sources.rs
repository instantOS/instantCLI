use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};

use crate::ui::prelude::Level;
use crate::video::audio::{PreprocessorType, create_preprocessor};
use crate::video::config::{VideoConfig, VideoDirectories};
use crate::video::document::VideoDocument;
use crate::video::document::{VideoMetadata, VideoSource};
use crate::video::render::logging::log_event;
use crate::video::render::paths;
use crate::video::support::utils::{canonicalize_existing, compute_file_hash};

pub(super) fn resolve_source_path(path: &Path, markdown_dir: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(markdown_dir.join(path))
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
    markdown_dir: &Path,
    config: &VideoConfig,
) -> Result<Vec<VideoSource>> {
    let sources = paths::resolve_video_sources(metadata, markdown_dir)?;
    let mut resolved = Vec::new();
    for source in sources {
        let resolved_source = resolve_source_path(&source.source, markdown_dir)?;
        let resolved_transcript = resolve_source_path(&source.transcript, markdown_dir)?;
        let resolved_audio = resolve_audio_path(&resolved_source, config).await?;
        let canonical = canonicalize_existing(&resolved_source)?;
        log_event(
            Level::Info,
            "video.render.video",
            format!("Using source {} video {}", source.id, canonical.display()),
        );
        resolved.push(VideoSource {
            source: resolved_source,
            transcript: resolved_transcript,
            audio: resolved_audio,
            ..source
        });
    }

    Ok(resolved)
}

async fn resolve_audio_path(video_path: &Path, config: &VideoConfig) -> Result<PathBuf> {
    log_event(
        Level::Info,
        "video.render.video.hash",
        "Computing hash for cache lookup",
    );

    let video_hash = compute_file_hash(video_path)?;
    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    let transcript_dir = project_paths.transcript_dir();

    // Check for local preprocessed file (WAV) - Preferred
    let local_processed_path = transcript_dir.join(format!("{}_local_processed.wav", video_hash));
    if local_processed_path.exists() {
        log_event(
            Level::Info,
            "video.render.audio",
            format!(
                "Using local preprocessed audio: {}",
                local_processed_path.display()
            ),
        );
        return Ok(local_processed_path);
    }

    // Check for Auphonic processed file (MP3) - Legacy/Alternative
    let auphonic_processed_path =
        transcript_dir.join(format!("{}_auphonic_processed.mp3", video_hash));

    if auphonic_processed_path.exists() {
        log_event(
            Level::Info,
            "video.render.audio",
            format!(
                "Using Auphonic processed audio: {}",
                auphonic_processed_path.display()
            ),
        );
        return Ok(auphonic_processed_path);
    }

    // No preprocessed audio found - run configured preprocessing
    let preprocessor = create_preprocessor(&config.preprocessor, config);

    // Skip preprocessing if configured to None
    if matches!(config.preprocessor, PreprocessorType::None) {
        log_event(
            Level::Info,
            "video.render.audio",
            "Preprocessing disabled. Using original video audio.",
        );
        return Ok(video_path.to_path_buf());
    }

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

    log_event(
        Level::Info,
        "video.render.audio.preprocess",
        format!(
            "No preprocessed audio found. Running {} preprocessing...",
            preprocessor.name()
        ),
    );

    let result = preprocessor.process(video_path, false).await?;

    log_event(
        Level::Success,
        "video.render.audio.preprocess",
        format!("Preprocessed audio ready: {}", result.output_path.display()),
    );

    Ok(result.output_path)
}

pub(super) async fn build_audio_source_map(
    sources: &[VideoSource],
    config: &VideoConfig,
) -> Result<HashMap<String, PathBuf>> {
    let mut audio_map = HashMap::new();
    for source in sources {
        let audio_path = resolve_audio_path(&source.source, config).await?;
        audio_map.insert(source.id.clone(), audio_path);
    }
    Ok(audio_map)
}
