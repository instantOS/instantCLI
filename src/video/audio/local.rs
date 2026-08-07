//! Local audio preprocessing using DeepFilterNet + ffmpeg-normalize
//!
//! Pipeline:
//! 1. Extract audio to WAV if input is video
//! 2. Run DeepFilterNet for noise reduction
//! 3. Run ffmpeg-normalize with podcast preset for loudness normalization

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::PreprocessCache;
use super::types::{AudioPreprocessor, PreprocessResult};
use crate::ui::prelude::{Level, emit};
use crate::video::support::DEEPFILTER_UVX_ARGS;
use crate::video::support::ffmpeg::extract_audio_to_wav;
use crate::video::support::utils::is_audio_file;

/// Local preprocessor using DeepFilterNet + ffmpeg-normalize
pub struct LocalPreprocessor;

impl LocalPreprocessor {
    pub fn new() -> Self {
        Self
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
            .args(DEEPFILTER_UVX_ARGS)
            .arg(&*input.to_string_lossy())
            .args([
                "--atten-lim",
                "80",   // More aggressive noise attenuation (higher = more reduction)
                "--pf", // Enable post-filter for additional noise reduction
                "--output-dir",
            ])
            .arg(&*output_dir.to_string_lossy())
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
    /// Uses aggressive dynamic compression for consistent speech levels
    fn run_normalize(input: &Path, output: &Path) -> Result<()> {
        emit(
            Level::Info,
            "video.preprocess.normalize",
            "Running aggressive loudness normalization with heavy compression...",
            None,
        );

        // AGGRESSIVE normalization settings for consistent speech volume:
        // - Target -10 LUFS: Louder overall level for speech (was -12)
        // - Loudness Range 1 LU: Very tight compression (was 3) - keeps volume consistent
        //   even when moving away from microphone
        // - True peak -1 dBTP: Prevent clipping
        let status = Command::new("uvx")
            .args([
                "ffmpeg-normalize",
                &input.to_string_lossy(),
                "--preset",
                "streaming-video",
                "--dynamic",
                "-lrt",
                "2",
                "--true-peak",
                "-1.0",
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
}

impl Default for LocalPreprocessor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AudioPreprocessor for LocalPreprocessor {
    async fn process(&self, input: &Path, force: bool) -> Result<PreprocessResult> {
        let cache = PreprocessCache::prepare(input)?;

        // Final output path (WAV to avoid lossy transcoding - encoding happens at render)
        let processed_cache_path = cache.path("local_processed.wav");

        // Check cache
        if let Some(cached) =
            cache.cached(&processed_cache_path, force, "video.preprocess.cached", "")
        {
            return Ok(cached);
        }

        // Step 1: Get audio as WAV
        let wav_path = cache.path("extracted.wav");
        if !wav_path.exists() || force {
            if !is_audio_file(input) {
                // Extract from video
                emit(
                    Level::Info,
                    "video.preprocess.extract",
                    &format!("Extracting audio from {}...", input.display()),
                    None,
                );
            }
            extract_audio_to_wav(input, &wav_path)?;
        }

        // Step 2: Run DeepFilterNet
        let denoised_path = Self::run_deepfilter(&wav_path, &cache.cache_dir)?;

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
