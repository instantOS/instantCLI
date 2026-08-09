//! Local audio enhancement using DeepFilterNet + podcast voice mastering
//!
//! Pipeline (voice stem, before music is mixed):
//! 1. Extract audio to WAV if input is video
//! 2. Run DeepFilterNet for noise reduction
//! 3. Apply podcast EQ, voice compression, and EBU R128 normalization
//!
//! Mastering is applied to the voice stem only. The music bed is mixed later and
//! passes through untouched, so mastered music is never double-compressed.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::EnhanceCache;
use super::types::{AudioEnhancer, EnhanceResult};
use crate::ui::prelude::{Level, emit};
use crate::video::support::DEEPFILTER_UVX_ARGS;
use crate::video::support::ffmpeg::{AudioExtractSpec, extract_audio};
use crate::video::support::utils::is_audio_file;

/// Local enhancer using DeepFilterNet + pure loudness normalization
pub struct LocalEnhancer;

impl LocalEnhancer {
    pub fn new() -> Self {
        Self
    }

    /// Run DeepFilterNet for noise reduction
    fn run_deepfilter(input: &Path, output_dir: &Path) -> Result<PathBuf> {
        emit(
            Level::Info,
            "video.enhance.deepfilter",
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
            "video.enhance.deepfilter",
            &format!("Noise reduction complete: {}", output_path.display()),
            None,
        );

        Ok(output_path)
    }
}

impl Default for LocalEnhancer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AudioEnhancer for LocalEnhancer {
    async fn enhance(&self, input: &Path, force: bool) -> Result<EnhanceResult> {
        let cache = EnhanceCache::prepare(input)?;

        // Enhanced stem for the render mix (WAV to avoid lossy transcoding;
        // encoding happens at render)
        let mastering_tag =
            &hex::encode(Sha256::digest(super::VOICE_MASTERING_RECIPE.as_bytes()))[..12];
        let enhanced_cache_path = cache.path(&format!("enhanced_deepfilter_{mastering_tag}.wav"));

        // Check cache
        if let Some(cached) = cache.cached(&enhanced_cache_path, force, "video.enhance.cached", "")
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
                    "video.enhance.extract",
                    &format!("Extracting audio from {}...", input.display()),
                    None,
                );
            }
            extract_audio(input, &wav_path, &AudioExtractSpec::MONO_48K_WAV)?;
        }

        // Step 2: Run DeepFilterNet
        let denoised_path = Self::run_deepfilter(&wav_path, &cache.cache_dir)?;

        // Step 3: Apply voice EQ, compression, and loudness normalization.
        super::run_loudnorm(&denoised_path, &enhanced_cache_path)?;

        Ok(EnhanceResult {
            output_path: enhanced_cache_path,
        })
    }

    fn name(&self) -> &'static str {
        "local"
    }

    fn is_available(&self) -> bool {
        which::which("uvx").is_ok() && which::which("ffmpeg").is_ok()
    }
}
