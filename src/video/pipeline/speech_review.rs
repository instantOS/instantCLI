use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::ui::prelude::{Level, emit};

use crate::video::support::ffmpeg::{AudioExtractSpec, extract_audio};
use crate::video::support::speech_activity::SpeechActivity;

use super::transcribe::{GRANITE_DRIVER_PYTHON, TRANSCRIBE_CPP_PACKAGE, download_file};

// Bump the cache suffix when the model or detector settings change. The
// caller's cache directory is keyed by the original recording's content hash.
const CACHE_FILENAME: &str = "speech-activity-silero-v6.2-v1.json";
const MODEL_FILENAME: &str = "silero-vad-v6.2-F32.gguf";
const MODEL_URL: &str = "https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/a9b2d61c20ae4ddf3644edb34969e8185a485448/silero-vad-v6.2-F32.gguf";

/// Optional, ASR-independent review of the raw source; never changes transcripts.
pub(super) fn review_audio(
    video_path: &Path,
    cache_dir: &Path,
    models_dir: &Path,
) -> Option<SpeechActivity> {
    let cache_path = cache_dir.join(CACHE_FILENAME);
    let cached = match fs::read_to_string(&cache_path) {
        Ok(json) => SpeechActivity::from_json(&json)
            .with_context(|| format!("Invalid speech activity cache {}", cache_path.display()))
            .map(Some),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to read speech activity cache {}",
                cache_path.display()
            )
        }),
    };
    match cached {
        Ok(Some(activity)) => return Some(activity),
        Ok(None) => {}
        Err(error) => emit(
            Level::Warn,
            "video.speech_review.cache_invalid",
            &format!("{error:#}; recomputing from source audio"),
            None,
        ),
    }

    emit(
        Level::Info,
        "video.speech_review.start",
        "Checking source audio for possible non-speech transcriptions (local Silero VAD)...",
        None,
    );
    match compute_review(video_path, &cache_path, models_dir) {
        Ok(activity) => Some(activity),
        Err(error) => {
            emit(
                Level::Warn,
                "video.speech_review.failed",
                &format!(
                    "Speech review unavailable for {}: {error:#}. Markdown will have no non-speech warnings; this does not mean the transcript is verified.",
                    video_path.display()
                ),
                None,
            );
            None
        }
    }
}

fn compute_review(
    video_path: &Path,
    cache_path: &Path,
    models_dir: &Path,
) -> Result<SpeechActivity> {
    let model_path = models_dir.join(MODEL_FILENAME);
    if !model_path.is_file() {
        download_file(MODEL_URL, &model_path, "Silero v6.2 speech activity model")
            .context("Failed to download speech review model")?;
    }

    let temporary = tempfile::tempdir().context("Failed to create speech review workspace")?;
    let wav_path = temporary.path().join("source.wav");
    extract_audio(video_path, &wav_path, &AudioExtractSpec::MONO_16K_WAV)
        .context("Failed to extract raw source audio for speech review")?;

    let output = Command::new("uv")
        .args([
            "run",
            "--no-project",
            "--python",
            GRANITE_DRIVER_PYTHON,
            "--with",
            TRANSCRIBE_CPP_PACKAGE,
            "--with",
            "numpy",
            "python",
            "-c",
            include_str!("speech_review.py"),
        ])
        .arg("--model")
        .arg(&model_path)
        .arg("--wav")
        .arg(&wav_path)
        .output()
        .context("Failed to run `uv run` for speech review")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        bail!(
            "Speech review driver exited with {}: {}",
            output.status,
            stderr.trim()
        );
    }
    let json = std::str::from_utf8(&output.stdout)
        .with_context(|| format!("Speech review returned non-UTF-8 output; stderr: {stderr}"))?;
    let activity = SpeechActivity::from_json(json).with_context(|| {
        format!("Speech review returned invalid activity JSON; stderr: {stderr}")
    })?;

    let parent = cache_path
        .parent()
        .context("Speech review cache has no parent")?;
    fs::create_dir_all(parent).context("Failed to create speech review cache directory")?;
    let mut cached = tempfile::NamedTempFile::new_in(parent)
        .context("Failed to create temporary speech review cache")?;
    cached
        .write_all(json.as_bytes())
        .context("Failed to write speech review cache")?;
    cached
        .flush()
        .context("Failed to flush speech review cache")?;
    cached
        .persist(cache_path)
        .context("Failed to persist speech review cache")?;
    Ok(activity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_cached_analysis_needs_neither_source_nor_model() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let cache_dir = temporary.path();
        fs::write(
            cache_dir.join(CACHE_FILENAME),
            r#"{"version":1,"duration_seconds":5,"speech":[]}"#,
        )?;
        let activity = review_audio(
            &cache_dir.join("missing-source.mp4"),
            cache_dir,
            &cache_dir.join("missing-models"),
        )
        .context("Expected valid cached speech review")?;
        assert!(
            activity.is_suspected_non_speech(
                std::time::Duration::ZERO,
                std::time::Duration::from_secs(1),
            )
        );
        assert!(!cache_dir.join("missing-models").exists());
        Ok(())
    }

    #[test]
    fn invalid_cache_and_unavailable_audio_are_not_evidence_of_silence() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let cache_dir = temporary.path();
        // Avoid any download: extraction must fail before this fixture is loaded.
        fs::write(cache_dir.join(MODEL_FILENAME), "unused fixture model")?;
        for cached in [
            "truncated",
            r#"{"version":99,"duration_seconds":5,"speech":[]}"#,
        ] {
            fs::write(cache_dir.join(CACHE_FILENAME), cached)?;
            assert!(
                review_audio(&cache_dir.join("missing-source.mp4"), cache_dir, cache_dir,)
                    .is_none()
            );
            assert_eq!(fs::read_to_string(cache_dir.join(CACHE_FILENAME))?, cached);
        }
        Ok(())
    }
}
