use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::prelude::{Level, emit};

use super::cli::ConvertArgs;
use super::config::{VideoDirectories, VideoProjectPaths};
use super::markdown::{MarkdownMetadata, build_markdown};
use super::srt::parse_srt;
use super::utils::{canonicalize_existing, compute_file_hash};

pub fn handle_convert(args: ConvertArgs) -> Result<()> {
    let video_path = canonicalize_existing(&args.video)?;
    let video_hash = compute_file_hash(&video_path)?;

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    project_paths.ensure_directories()?;

    let cached_transcript_path = project_paths.transcript_cache_path().to_path_buf();
    let cached_exists = cached_transcript_path.exists();

    let transcript_path = if let Some(provided) = &args.transcript {
        let provided_path = canonicalize_existing(provided)?;
        copy_transcript(&provided_path, &cached_transcript_path)?;
        cached_transcript_path.clone()
    } else {
        if !cached_exists {
            anyhow::bail!(
                "Transcript not found at {}. Run `ins video transcribe` or pass --transcript to provide one.",
                cached_transcript_path.display()
            );
        }
        cached_transcript_path.clone()
    };

    let transcript_contents = fs::read_to_string(&transcript_path)
        .with_context(|| format!("Failed to read transcript at {}", transcript_path.display()))?;

    let cues = parse_srt(&transcript_contents)?;

    let video_name = video_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .context("Video file name is not valid UTF-8")?;

    let metadata = MarkdownMetadata {
        video_hash: video_hash.as_str(),
        video_name: video_name.as_str(),
        video_source: &video_path,
        transcript_source: &transcript_path,
    };

    let markdown = build_markdown(&cues, &metadata);

    let output_path = determine_output_path(args.output, &project_paths)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    fs::write(&output_path, markdown.as_bytes())
        .with_context(|| format!("Failed to write markdown file to {}", output_path.display()))?;

    write_metadata_file(
        &project_paths,
        &video_hash,
        &video_path,
        &transcript_path,
        &output_path,
    )?;

    emit(
        Level::Success,
        "video.convert.success",
        &format!("Generated markdown at {}", output_path.display()),
        None,
    );
    emit(
        Level::Info,
        "video.convert.cached",
        &format!(
            "Cached transcript at {}",
            project_paths.transcript_cache_path().display()
        ),
        None,
    );

    Ok(())
}
fn copy_transcript(src: &Path, dest: &Path) -> Result<()> {
    if src == dest {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create transcript cache directory {}",
                parent.display()
            )
        })?;
    }
    fs::copy(src, dest).with_context(|| {
        format!(
            "Failed to copy transcript from {} to {}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn determine_output_path(
    output: Option<PathBuf>,
    project_paths: &VideoProjectPaths,
) -> Result<PathBuf> {
    Ok(output.unwrap_or_else(|| project_paths.markdown_path().to_path_buf()))
}

fn write_metadata_file(
    project_paths: &VideoProjectPaths,
    video_hash: &str,
    video_path: &Path,
    transcript_path: &Path,
    markdown_path: &Path,
) -> Result<()> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    let contents = format!(
        "video_hash: {hash}\nvideo_source: {video}\ntranscript_source: {transcript}\nmarkdown: {markdown}\nupdated_at: '{timestamp}'\n",
        hash = yaml_quote(video_hash),
        video = yaml_quote_path(video_path),
        transcript = yaml_quote_path(transcript_path),
        markdown = yaml_quote_path(markdown_path),
    );

    fs::write(project_paths.metadata_path(), contents).with_context(|| {
        format!(
            "Failed to write metadata file to {}",
            project_paths.metadata_path().display()
        )
    })?;
    Ok(())
}

fn yaml_quote(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

fn yaml_quote_path(path: &Path) -> String {
    yaml_quote(&path.to_string_lossy())
}
