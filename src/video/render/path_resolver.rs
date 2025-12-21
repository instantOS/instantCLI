use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::video::config::VideoDirectories;
use crate::video::document::{VideoMetadata, VideoMetadataVideo};

pub fn resolve_video_path(metadata: &VideoMetadata, markdown_dir: &Path) -> Result<PathBuf> {
    let video_meta: &VideoMetadataVideo = metadata
        .video
        .as_ref()
        .ok_or_else(|| anyhow!("Front matter is missing `video` metadata"))?;

    if let Some(source) = &video_meta.source {
        let resolved = if source.is_absolute() {
            source.clone()
        } else {
            markdown_dir.join(source)
        };
        return Ok(resolved);
    }

    if let Some(name) = &video_meta.name {
        return Ok(markdown_dir.join(name));
    }

    Err(anyhow!(
        "Front matter must include either `video.source` or `video.name` to locate the source video"
    ))
}

pub fn resolve_transcript_path(metadata: &VideoMetadata, markdown_dir: &Path) -> Result<PathBuf> {
    let transcript_meta = metadata
        .transcript
        .as_ref()
        .ok_or_else(|| anyhow!("Front matter is missing `transcript` metadata"))?;

    let source = transcript_meta
        .source
        .as_ref()
        .ok_or_else(|| anyhow!("Front matter is missing `transcript.source`"))?;

    let resolved = if source.is_absolute() {
        source.clone()
    } else {
        markdown_dir.join(source)
    };

    if resolved.exists() {
        return Ok(resolved);
    }

    let Some(video_meta) = metadata.video.as_ref() else {
        return Ok(resolved);
    };
    let Some(hash) = video_meta.hash.as_ref() else {
        return Ok(resolved);
    };

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(hash);
    let cached_transcript = project_paths.transcript_cache_path();

    if cached_transcript.exists() {
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create transcript directory {}", parent.display())
            })?;
        }

        fs::copy(cached_transcript, &resolved).with_context(|| {
            format!(
                "Failed to copy cached transcript from {} to {}",
                cached_transcript.display(),
                resolved.display()
            )
        })?;
    }

    Ok(resolved)
}

pub fn resolve_output_path(
    out_file: Option<&PathBuf>,
    video_path: &Path,
    markdown_dir: &Path,
) -> Result<PathBuf> {
    if let Some(provided) = out_file {
        let resolved = if provided.is_absolute() {
            provided.clone()
        } else {
            markdown_dir.join(provided)
        };
        return Ok(resolved);
    }

    let mut output = video_path.to_path_buf();
    let stem = video_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("Video path {} has no valid file name", video_path.display()))?;
    output.set_file_name(format!("{stem}_edit.mp4"));
    Ok(output)
}
