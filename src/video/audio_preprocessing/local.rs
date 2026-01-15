//! Local audio preprocessing using DeepFilterNet + ffmpeg-normalize
//!
//! Pipeline:
//! 1. Extract audio to WAV if input is video
//! 2. Run DeepFilterNet for noise reduction
//! 3. Run ffmpeg-normalize with podcast preset for loudness normalization

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::types::{AudioPreprocessor, PreprocessResult};
use crate::ui::prelude::{Level, emit};
use crate::video::config::VideoDirectories;
use crate::video::utils::compute_file_hash;

/// Local preprocessor using DeepFilterNet + ffmpeg-normalize
pub struct LocalPreprocessor;

impl LocalPreprocessor {
    pub fn new() -> Self {
        Self
    }

    /// Extract audio to WAV format for DeepFilterNet processing
    fn extract_audio_to_wav(input: &Path, output: &Path) -> Result<()> {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                &input.to_string_lossy(),
                "-vn",
                "-map",
                "0:a:0",
                "-ac",
                "1", // Downmix to mono
                "-c:a",
                "pcm_s16le",
                "-ar",
                "48000",
                &output.to_string_lossy(),
            ])
            .status()
            .with_context(|| {
                format!(
                    "Failed to run ffmpeg to extract audio from {}",
                    input.display()
                )
            })?;

        if !status.success() {
            anyhow::bail!("ffmpeg failed to extract audio from {}", input.display());
        }

        Ok(())
    }

    /// Run DeepFilterNet for noise reduction
    fn run_deepfilter(input: &Path, output_dir: &Path) -> Result<PathBuf> {
        emit(
            Level::Info,
            "video.preprocess.deepfilter",
            "Running DeepFilterNet noise reduction...",
            None,
        );

        let status = Command::new("uvx")
            .args([
                "--python",
                "3.10",
                "--from",
                "deepfilternet",
                "--with",
                "torch<2.1",
                "--with",
                "torchaudio<2.1",
                "deepFilter",
                &input.to_string_lossy(),
                "--atten-lim",
                "80",   // More aggressive noise attenuation (higher = more reduction)
                "--pf", // Enable post-filter for additional noise reduction
                "--output-dir",
                &output_dir.to_string_lossy(),
            ])
            .status()
            .context("Failed to run DeepFilterNet")?;

        if !status.success() {
            anyhow::bail!("DeepFilterNet failed to process {}", input.display());
        }

        // DeepFilterNet outputs to <output_dir>/<input_stem>_DeepFilterNet3_pf.wav when --pf is used
        let input_stem = input.file_stem().unwrap_or_default().to_string_lossy();
        let output_path = output_dir.join(format!("{}_DeepFilterNet3_pf.wav", input_stem));

        if !output_path.exists() {
            anyhow::bail!(
                "DeepFilterNet output not found at expected path: {}",
                output_path.display()
            );
        }

        emit(
            Level::Success,
            "video.preprocess.deepfilter",
            &format!("Noise reduction complete: {}", output_path.display()),
            None,
        );

        Ok(output_path)
    }

    /// Run ffmpeg-normalize for loudness normalization
    /// Uses dynamic compression for consistent speech levels + YouTube loudness target
    fn run_normalize(input: &Path, output: &Path) -> Result<()> {
        emit(
            Level::Info,
            "video.preprocess.normalize",
            "Running loudness normalization with compression...",
            None,
        );

        // Dynamic normalization: compresses audio to reduce volume fluctuations
        // Good for speech where mic distance varies
        // Using louder target for spoken content + more compression for consistency
        let status = Command::new("uvx")
            .args([
                "ffmpeg-normalize",
                &input.to_string_lossy(),
                "-nt",
                "ebu", // EBU R128 normalization
                "-t",
                "-12", // Louder target for speech: -12 LUFS (was -14)
                "-tp",
                "-1", // True peak: -1 dBTP
                "-lrt",
                "3", // Even more compression: 3 LU (was 4) for very consistent volume
                "-o",
                &output.to_string_lossy(),
                "-f", // Force overwrite
            ])
            .status()
            .context("Failed to run ffmpeg-normalize")?;

        if !status.success() {
            anyhow::bail!("ffmpeg-normalize failed to process {}", input.display());
        }

        emit(
            Level::Success,
            "video.preprocess.normalize",
            &format!("Loudness normalization complete: {}", output.display()),
            None,
        );

        Ok(())
    }

    /// Check if input is an audio file
    fn is_audio_file(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                ["mp3", "wav", "flac", "m4a", "ogg", "aac", "wma", "aiff"]
                    .contains(&e.to_lowercase().as_str())
            })
            .unwrap_or(false)
    }
}

impl Default for LocalPreprocessor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AudioPreprocessor for LocalPreprocessor {
    async fn process(&self, input: &Path, force: bool) -> Result<PreprocessResult> {
        let input_hash = compute_file_hash(input)?;

        let directories = VideoDirectories::new()?;
        let project_paths = directories.project_paths(&input_hash);
        project_paths.ensure_directories()?;

        let cache_dir = project_paths.transcript_dir();

        // Final output path (WAV to avoid lossy transcoding - encoding happens at render)
        let processed_cache_path = cache_dir.join(format!("{}_local_processed.wav", input_hash));

        // Check cache
        if processed_cache_path.exists() && !force {
            emit(
                Level::Info,
                "video.preprocess.cached",
                &format!("Using cached result: {}", processed_cache_path.display()),
                None,
            );
            return Ok(PreprocessResult {
                output_path: processed_cache_path,
            });
        }

        // Step 1: Get audio as WAV
        let wav_path = cache_dir.join(format!("{}_extracted.wav", input_hash));
        if !wav_path.exists() || force {
            if Self::is_audio_file(input) {
                // Convert audio to WAV
                Self::extract_audio_to_wav(input, &wav_path)?;
            } else {
                // Extract from video
                emit(
                    Level::Info,
                    "video.preprocess.extract",
                    &format!("Extracting audio from {}...", input.display()),
                    None,
                );
                Self::extract_audio_to_wav(input, &wav_path)?;
            }
        }

        // Step 2: Run DeepFilterNet
        let denoised_path = Self::run_deepfilter(&wav_path, cache_dir)?;

        // Step 3: Run ffmpeg-normalize
        Self::run_normalize(&denoised_path, &processed_cache_path)?;

        Ok(PreprocessResult {
            output_path: processed_cache_path,
        })
    }

    fn name(&self) -> &'static str {
        "local"
    }

    fn is_available(&self) -> bool {
        which::which("uvx").is_ok() && which::which("ffmpeg").is_ok()
    }
}
