use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::prelude::{Level, emit};

use super::auphonic::process_with_auphonic;
use super::cli::{ConvertArgs, TranscribeArgs};
use super::config::{VideoConfig, VideoDirectories, VideoProjectPaths};
use super::markdown::{MarkdownMetadata, build_markdown};
use super::srt::parse_srt;
use super::transcribe::handle_transcribe;
use super::utils::{canonicalize_existing, compute_file_hash};

//TODO: this is a big function, consider breaking it up
pub async fn handle_convert(args: ConvertArgs) -> Result<()> {
    emit(
        Level::Info,
        "video.convert.start",
        &format!("Analyzing video {}...", args.video.display()),
        None,
    );
    let video_path = canonicalize_existing(&args.video)?;
    let video_hash = compute_file_hash(&video_path)?;

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    project_paths.ensure_directories()?;

    let output_path = determine_output_path(args.out_file.clone(), &video_path)?;
    let markdown_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    let subtitle_dir = markdown_dir.join("insvideodata");
    let subtitle_output_path = subtitle_dir.join(format!("{video_hash}.srt"));
    let relative_subtitle_path = Path::new("./insvideodata").join(format!("{video_hash}.srt"));

    if output_path.exists() && !args.force {
        anyhow::bail!(
            "Markdown file already exists at {}. Use --force to overwrite.",
            output_path.display()
        );
    }

    let cached_transcript_path = project_paths.transcript_cache_path().to_path_buf();

    if let Some(provided) = &args.transcript {
        let provided_path = canonicalize_existing(provided)?;
        copy_transcript(&provided_path, &cached_transcript_path)?;
    } else if !cached_transcript_path.exists() {
        let config = VideoConfig::load()?;
        let auphonic_enabled = config.auphonic_enabled && !args.no_auphonic;

        let audio_source = if auphonic_enabled {
            // Process with Auphonic first
            // We don't have CLI args for api_key/preset here, so we rely on config
            match process_with_auphonic(&video_path, args.force, None, None).await {
                Ok(path) => path,
                Err(e) => {
                    emit(
                        Level::Warn,
                        "video.convert.auphonic_failed",
                        &format!(
                            "Auphonic processing failed: {}. Falling back to original video.",
                            e
                        ),
                        None,
                    );
                    video_path.clone()
                }
            }
        } else {
            video_path.clone()
        };

        emit(
            Level::Info,
            "video.convert.transcribe",
            "Transcribing audio (this may take a while)...",
            None,
        );

        handle_transcribe(TranscribeArgs {
            video: audio_source.clone(),
            compute_type: "int8".to_string(),
            device: "cpu".to_string(),
            model: None,
            vad_method: "silero".to_string(),
            force: false,
        })?;

        // If we transcribed a processed audio file (different from video_path),
        // the transcript will be stored under the audio file's hash.
        // We need to move it to the video project's transcript path.
        if audio_source != video_path {
            let audio_hash = compute_file_hash(&audio_source)?;
            let audio_project_paths = directories.project_paths(&audio_hash);
            let generated_transcript = audio_project_paths.transcript_cache_path();

            if generated_transcript.exists() {
                emit(
                    Level::Debug,
                    "video.convert.relocate",
                    &format!(
                        "Moving transcript from {} to {}",
                        generated_transcript.display(),
                        cached_transcript_path.display()
                    ),
                    None,
                );
                copy_transcript(generated_transcript, &cached_transcript_path)?;
            }
        }
    }

    let transcript_path = cached_transcript_path.clone();

    if !transcript_path.exists() {
        anyhow::bail!(
            "Transcript not found at {} even after attempting transcription.",
            transcript_path.display()
        );
    }

    copy_transcript(&transcript_path, &subtitle_output_path)?;

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
        transcript_source: &relative_subtitle_path,
    };

    let markdown = build_markdown(&cues, &metadata);

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
        &subtitle_output_path,
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
        "video.convert.subtitles",
        &format!("Stored subtitles at {}", subtitle_output_path.display()),
        None,
    );

    Ok(())
}
fn copy_transcript(src: &Path, dest: &Path) -> Result<()> {
    if src == dest {
        return Ok(());
    }
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create transcript directory {}", parent.display())
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

fn determine_output_path(output: Option<PathBuf>, video_path: &Path) -> Result<PathBuf> {
    match output {
        Some(path) => Ok(path),
        None => {
            // Default to <videoname>.md next to the video file
            let video_stem = video_path
                .file_stem()
                .context("Video file has no stem")?
                .to_string_lossy();
            let mut default_output = video_path.to_path_buf();
            default_output.set_file_name(format!("{}.video.md", video_stem));
            Ok(default_output)
        }
    }
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
