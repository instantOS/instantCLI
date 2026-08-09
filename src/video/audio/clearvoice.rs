//! ClearVoice (Alibaba DAMO) AI speech enhancement backend.
//!
//! This is the default enhancer: fullband 48 kHz speech enhancement
//! (denoise + restoration) using MossFormer2_SE_48K, followed by the same
//! dynamic EBU R128 loudness step as the Local backend.
//!
//! The heavy Python stack (torch + clearvoice) runs via `uv run --with`,
//! matching the Granite/whisperx driver pattern. Model checkpoints are
//! auto-downloaded from Hugging Face into `<models>/checkpoints` on first use.
//!
//! Pipeline (voice stem, before music is mixed):
//! 1. Extract audio to WAV if input is video
//! 2. ClearVoice AI enhancement (denoise + restoration)
//! 3. Dynamic two-pass EBU R128 loudness normalization

use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use super::EnhanceCache;
use super::types::{AudioEnhancer, EnhanceResult};
use crate::ui::prelude::{Level, emit};
use crate::video::config::VideoDirectories;
use crate::video::support::ffmpeg::{AudioExtractSpec, extract_audio};
use crate::video::support::utils::is_audio_file;

/// Speech enhancement model name (fullband 48 kHz, best quality).
const CLEARVOICE_MODEL: &str = "MossFormer2_SE_48K";
const CLEARVOICE_PACKAGE: &str = "clearvoice==0.1.2";
const CLEARVOICE_DRIVER: &str = include_str!("clearvoice_driver.py");
const CLEARVOICE_POSTPROCESS_RECIPE: &str =
    "ffmpeg-normalize:streaming-video:dynamic:lrt=2:true-peak=-1:sample-rate=48000";

fn cache_tag(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hex::encode(hasher.finalize())[..12].to_string()
}

fn driver_cache_tag() -> String {
    cache_tag(&[CLEARVOICE_PACKAGE.as_bytes(), CLEARVOICE_DRIVER.as_bytes()])
}

fn enhanced_cache_tag() -> String {
    cache_tag(&[
        CLEARVOICE_DRIVER.as_bytes(),
        CLEARVOICE_PACKAGE.as_bytes(),
        CLEARVOICE_POSTPROCESS_RECIPE.as_bytes(),
    ])
}

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
        let driver_is_current = std::fs::read_to_string(&driver_path)
            .map(|contents| contents == CLEARVOICE_DRIVER)
            .unwrap_or(false);
        if !driver_is_current {
            std::fs::write(&driver_path, CLEARVOICE_DRIVER)?;
        }

        let status = std::process::Command::new("uv")
            .args(["run", "--with", CLEARVOICE_PACKAGE, "python"])
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
        let enhanced_cache_path =
            cache.path(&format!("enhanced_clearvoice_{}.wav", enhanced_cache_tag()));

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
            extract_audio(input, &wav_path, &AudioExtractSpec::MONO_48K_WAV)?;
        }

        // Step 2: ClearVoice AI enhancement (denoise + restoration)
        let cv_raw_path = cache.path(&format!("clearvoice_raw_{}.wav", driver_cache_tag()));
        if !cv_raw_path.exists() || force {
            Self::run_driver(&wav_path, &cv_raw_path)?;
        }

        // Step 3: Static loudness normalization (no compression)
        super::run_loudnorm(&cv_raw_path, &enhanced_cache_path)?;

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
