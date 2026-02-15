use anyhow::{Context, Result};
use duct::cmd;
use std::fs;
use std::path::Path;

use crate::ui::prelude::{Level, emit};

use crate::video::cli::TranscribeArgs;
use crate::video::config::VideoDirectories;
use crate::video::support::utils::{
    canonicalize_existing, compute_file_hash, extension_or_default,
};

pub fn handle_transcribe(args: TranscribeArgs) -> Result<()> {
    let video_path = canonicalize_existing(&args.video)?;
    emit(
        Level::Info,
        "video.transcribe.start",
        &format!("Starting transcription for {}...", video_path.display()),
        None,
    );
    let video_hash = compute_file_hash(&video_path)?;

    let directories = VideoDirectories::new()?;
    let cache_paths = directories.cache_paths(&video_hash);
    cache_paths.ensure_directories()?;

    let transcript_path = cache_paths.transcript_cache_path().to_path_buf();
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
    let hashed_video_path = cache_paths.hashed_video_input(&extension);
    prepare_hashed_video_input(&video_path, &hashed_video_path)?;

    let run_result = run_whisperx(&hashed_video_path, cache_paths.transcript_dir(), &args);

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
    let hashed_video = hashed_video.to_string_lossy();
    let output_dir = output_dir.to_string_lossy();

    let mut whisper_args: Vec<&str> = vec![
        "whisperx",
        &hashed_video,
        "--output_format",
        "json",
        "--output_dir",
        &output_dir,
        "--vad_method",
        &args.vad_method,
        "--compute_type",
        &args.compute_type,
        "--device",
        &args.device,
        "--align_model",
        "WAV2VEC2_ASR_LARGE_LV60K_960H",
        "--batch_size",
        "4",
        "--segment_resolution",
        "chunk",
        "--beam_size",
        "5",
        "--patience",
        "1.0",
        "--max_line_width",
        "42",
        "--threads",
        "8",
    ];

    if let Some(model) = &args.model {
        whisper_args.push("--model");
        whisper_args.push(model);
    }

    cmd("uvx", &whisper_args)
        .run()
        .with_context(|| format!("Failed to run WhisperX for {}", hashed_video))?;

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
