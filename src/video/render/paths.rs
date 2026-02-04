use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::RenderMode;
use crate::video::document::{VideoMetadata, VideoSource};

pub fn resolve_video_sources(
    metadata: &VideoMetadata,
    markdown_dir: &Path,
) -> Result<Vec<VideoSource>> {
    let mut resolved = Vec::new();

    for source in &metadata.sources {
        let video_path = if source.source.is_absolute() {
            source.source.clone()
        } else {
            markdown_dir.join(&source.source)
        };
        let transcript_path = if source.transcript.is_absolute() {
            source.transcript.clone()
        } else {
            markdown_dir.join(&source.transcript)
        };

        resolved.push(VideoSource {
            id: source.id.clone(),
            name: source.name.clone(),
            source: video_path,
            transcript: transcript_path,
            hash: source.hash.clone(),
        });
    }

    Ok(resolved)
}

pub fn resolve_output_path(
    out_file: Option<&PathBuf>,
    video_path: &Path,
    markdown_dir: &Path,
    render_mode: RenderMode,
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

    let suffix = render_mode.output_suffix();
    output.set_file_name(format!("{stem}{suffix}.mp4"));
    Ok(output)
}
