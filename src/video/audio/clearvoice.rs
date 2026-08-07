//! ClearVoice (Alibaba DAMO) AI speech enhancement backend.
//!
//! This is the default enhancer: fullband 48 kHz speech enhancement
//! (denoise + restoration) using MossFormer2_SE_48K, followed by the same
//! pure static EBU R128 loudness step as the Local backend.
//!
//! The heavy Python stack (torch + clearvoice) runs via `uv run --with`,
//! matching the Granite/whisperx driver pattern. Model checkpoints are
//! auto-downloaded from Hugging Face into `<models>/checkpoints` on first use.
//!
//! Pipeline (voice stem, before music is mixed):
//! 1. Extract audio to WAV if input is video
//! 2. ClearVoice AI enhancement (denoise + restoration)
//! 3. Static two-pass EBU R128 loudness normalization (no compression)

use std::path::Path;

use anyhow::{Context, Result};

use super::EnhanceCache;
use super::types::{AudioEnhancer, EnhanceResult};
use crate::ui::prelude::{Level, emit};
use crate::video::config::VideoDirectories;
use crate::video::support::ffmpeg::extract_audio_to_wav;
use crate::video::support::utils::is_audio_file;

/// Speech enhancement model name (fullband 48 kHz, best quality).
const CLEARVOICE_MODEL: &str = "MossFormer2_SE_48K";

/// ClearVoice enhancer driven by `uv run --with clearvoice`.
pub struct ClearVoiceEnhancer;

impl ClearVoiceEnhancer {
    pub fn new() -> Self {
        Self
    }

    /// Run the embedded driver: AI enhancement, cwd = models dir so
    /// `./checkpoints/...` (auto-downloaded once) lands next to the venv.
    fn run_driver(input: &Path, output: &Path) -> Result<()> {
        emit(
            Level::Info,
            "video.enhance.clearvoice.model",
            &format!("Running ClearVoice {} enhancement...", CLEARVOICE_MODEL),
            None,
        );

        let directories = VideoDirectories::new()?;
        let models_dir = directories.models_dir();
        std::fs::create_dir_all(&models_dir)?;

        let driver_path = models_dir.join("clearvoice_driver.py");
        if !driver_path.exists() {
            std::fs::write(&driver_path, include_str!("clearvoice_driver.py"))?;
        }

        let status = std::process::Command::new("uv")
            .args(["run", "--with", "clearvoice", "python"])
            .arg(&driver_path)
            .arg(input)
            .arg(output)
            .current_dir(&models_dir)
            .env_remove("MPLBACKEND")
            .status()
            .with_context(|| {
                format!(
                    "Failed to run ClearVoice driver at {}",
                    driver_path.display()
                )
            })?;

        if !status.success() {
            anyhow::bail!("ClearVoice failed to enhance {}", input.display());
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AudioEnhancer for ClearVoiceEnhancer {
    async fn enhance(&self, input: &Path, force: bool) -> Result<EnhanceResult> {
        let cache = EnhanceCache::prepare(input)?;

        // Enhanced stem for the render mix. Engine-tagged so switching
        // backends never serves a stale result produced by another engine.
        let enhanced_cache_path = cache.path("enhanced_clearvoice.wav");

        // Check cache
        if let Some(cached) = cache.cached(&enhanced_cache_path, force, "video.enhance.cached", "")
        {
            return Ok(cached);
        }

        // Step 1: Get audio as WAV
        let wav_path = cache.path("extracted.wav");
        if !wav_path.exists() || force {
            if !is_audio_file(input) {
                emit(
                    Level::Info,
                    "video.enhance.extract",
                    &format!("Extracting audio from {}...", input.display()),
                    None,
                );
            }
            extract_audio_to_wav(input, &wav_path)?;
        }

        // Step 2: ClearVoice AI enhancement (denoise + restoration)
        let cv_raw_path = cache.path("clearvoice_raw.wav");
        if !cv_raw_path.exists() || force {
            Self::run_driver(&wav_path, &cv_raw_path)?;
        }

        // Step 3: Static loudness normalization (no compression)
        super::run_static_loudnorm(&cv_raw_path, &enhanced_cache_path)?;

        Ok(EnhanceResult {
            output_path: enhanced_cache_path,
        })
    }

    fn name(&self) -> &'static str {
        "clearvoice"
    }

    fn is_available(&self) -> bool {
        which::which("uv").is_ok() && which::which("ffmpeg").is_ok()
    }
}
