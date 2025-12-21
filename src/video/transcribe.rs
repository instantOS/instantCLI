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
    emit(
        Level::Info,
        "video.transcribe.start",
        &format!("Starting transcription for {}...", video_path.display()),
        None,
    );
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
    let mut whisper_args: Vec<OsString> = vec![
        OsString::from("whisperx"),
        hashed_video.as_os_str().to_os_string(),
        OsString::from("--output_format"),
        OsString::from("srt"),
        OsString::from("--output_dir"),
        output_dir.as_os_str().to_os_string(),
        OsString::from("--vad_method"),
        OsString::from(args.vad_method.clone()),
        OsString::from("--compute_type"),
        OsString::from(args.compute_type.clone()),
        OsString::from("--device"),
        OsString::from(args.device.clone()),
        // Improved alignment quality parameters for CPU processing
        OsString::from("--align_model"),
        OsString::from("WAV2VEC2_ASR_LARGE_LV60K_960H"), // Best alignment model for accurate word-level timestamps
        OsString::from("--batch_size"),
        OsString::from("4"), // Smaller batch size optimized for CPU (vs 16 for GPU)
        OsString::from("--segment_resolution"),
        OsString::from("sentence"), // Sentence-level segmentation for better subtitle timing
        OsString::from("--beam_size"),
        OsString::from("5"), // Beam search for higher transcription accuracy
        OsString::from("--patience"),
        OsString::from("1.0"), // Beam search patience for thorough exploration
        OsString::from("--max_line_width"),
        OsString::from("42"), // Optimal characters per subtitle line
        OsString::from("--threads"),
        OsString::from("8"), // CPU thread optimization
    ];

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
