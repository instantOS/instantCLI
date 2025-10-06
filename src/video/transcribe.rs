use anyhow::{Context, Result};
use duct::cmd;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use crate::ui::prelude::{Level, emit};

use super::cli::TranscribeArgs;
use super::config::VideoDirectories;
use super::utils::{canonicalize_existing, compute_file_hash, extension_or_default};

pub fn handle_transcribe(args: TranscribeArgs) -> Result<()> {
    let video_path = canonicalize_existing(&args.video)?;
    let video_hash = compute_file_hash(&video_path)?;

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    project_paths.ensure_directories()?;

    let transcript_path = project_paths.transcript_cache_path().to_path_buf();
    if transcript_path.exists() && !args.force {
        emit(
            Level::Info,
            "video.transcribe.cached",
            &format!(
                "Transcript already cached at {} (use --force to regenerate)",
                transcript_path.display()
            ),
            None,
        );
        return Ok(());
    }

    let extension = extension_or_default(&video_path, "mp4");
    let hashed_video_path = project_paths.hashed_video_input(&extension);
    prepare_hashed_video_input(&video_path, &hashed_video_path)?;

    let run_result = run_whisperx(&hashed_video_path, project_paths.transcript_dir(), &args);

    // Clean up temporary copy regardless of success
    if let Err(err) = cleanup_hashed_video_input(&hashed_video_path) {
        emit(
            Level::Warn,
            "video.transcribe.cleanup_failed",
            &format!(
                "Failed to remove temporary file {}: {}",
                hashed_video_path.display(),
                err
            ),
            None,
        );
    }

    run_result?;

    if !transcript_path.exists() {
        anyhow::bail!(
            "WhisperX did not produce the expected transcript at {}",
            transcript_path.display()
        );
    }

    emit(
        Level::Success,
        "video.transcribe.success",
        &format!("Generated transcript at {}", transcript_path.display()),
        None,
    );

    Ok(())
}

fn run_whisperx(hashed_video: &Path, output_dir: &Path, args: &TranscribeArgs) -> Result<()> {
    let mut whisper_args: Vec<OsString> = Vec::new();
    whisper_args.push(OsString::from("whisperx"));
    whisper_args.push(hashed_video.as_os_str().to_os_string());
    whisper_args.push(OsString::from("--output_format"));
    whisper_args.push(OsString::from("srt"));
    whisper_args.push(OsString::from("--output_dir"));
    whisper_args.push(output_dir.as_os_str().to_os_string());
    whisper_args.push(OsString::from("--compute_type"));
    whisper_args.push(OsString::from(args.compute_type.clone()));
    whisper_args.push(OsString::from("--device"));
    whisper_args.push(OsString::from(args.device.clone()));

    if let Some(model) = &args.model {
        whisper_args.push(OsString::from("--model"));
        whisper_args.push(OsString::from(model.clone()));
    }

    cmd("uvx", whisper_args)
        .run()
        .with_context(|| format!("Failed to run WhisperX for {}", hashed_video.display()))?;

    Ok(())
}

fn prepare_hashed_video_input(source: &Path, hashed_path: &Path) -> Result<()> {
    if hashed_path.exists() {
        fs::remove_file(hashed_path).with_context(|| {
            format!(
                "Failed to remove existing temporary file {}",
                hashed_path.display()
            )
        })?;
    }

    if let Some(parent) = hashed_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create directory {} for temporary video",
                parent.display()
            )
        })?;
    }

    fs::copy(source, hashed_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            source.display(),
            hashed_path.display()
        )
    })?;

    Ok(())
}

fn cleanup_hashed_video_input(hashed_path: &Path) -> Result<()> {
    if hashed_path.exists() {
        fs::remove_file(hashed_path).with_context(|| {
            format!("Failed to remove temporary file {}", hashed_path.display())
        })?;
    }
    Ok(())
}
