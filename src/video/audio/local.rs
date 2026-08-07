//! Local audio enhancement using DeepFilterNet + pure EBU R128 loudness
//!
//! Pipeline (voice stem, before music is mixed):
//! 1. Extract audio to WAV if input is video
//! 2. Run DeepFilterNet for noise reduction
//! 3. Run a static two-pass EBU R128 loudness normalization (NO compressor)
//!
//! The loudness step deliberately avoids dynamic range processing: `linear=true`
//! applies a single linear gain, so the voice keeps its dynamics and the music
//! (mixed later, after this stage) is never double-compressed.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::EnhanceCache;
use super::types::{AudioEnhancer, EnhanceResult};
use crate::ui::prelude::{Level, emit};
use crate::video::support::DEEPFILTER_UVX_ARGS;
use crate::video::support::ffmpeg::extract_audio_to_wav;
use crate::video::support::utils::is_audio_file;

/// Loudness normalization targets (EBU R128, static gain only).
/// - Integrated target: -14 LUFS (standard for internet video)
/// - Loudness range: 11 LU (no hard squeezing of the voice)
/// - True peak: -1.0 dBTP headroom
const LOUDNORM_TARGET_LUFS: &str = "-14";
const LOUDNORM_TRUE_PEAK: &str = "-1.0";
const LOUDNORM_LRA: &str = "11";

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

    /// Run a pure two-pass EBU R128 loudness normalization with a STATIC gain.
    ///
    /// Pass 1 measures the source with `print_format=json`; pass 2 applies the
    /// measured parameters with `linear=true`, so ffmpeg rescales by a constant
    /// factor instead of running its dynamic compressor/limiter. The voice keeps
    /// its natural dynamics; only overall level moves toward the target.
    fn run_loudnorm(input: &Path, output: &Path) -> Result<()> {
        emit(
            Level::Info,
            "video.enhance.loudnorm",
            "Running pure EBU R128 loudness normalization (no compression)...",
            None,
        );

        // Pass 1: measure loudness (output printed as JSON on stderr)
        let measure = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-nostdin",
                "-i",
                &input.to_string_lossy(),
                "-af",
                &format!(
                    "loudnorm=I={}:TP={}:LRA={}:print_format=json",
                    LOUDNORM_TARGET_LUFS, LOUDNORM_TRUE_PEAK, LOUDNORM_LRA
                ),
                "-f",
                "null",
                "-",
            ])
            .output()
            .context("Failed to run ffmpeg loudness measurement")?;

        if !measure.status.success() {
            anyhow::bail!("Loudness measurement failed for {}", input.display());
        }

        let stderr = String::from_utf8_lossy(&measure.stderr);
        let json_start = stderr.find('{').ok_or_else(|| {
            anyhow!(
                "ffmpeg loudnorm printed no JSON on stderr for {}",
                input.display()
            )
        })?;
        let json_text = &stderr[json_start..];
        let json_end = json_text
            .find("}")
            .map(|i| i + 1)
            .ok_or_else(|| anyhow!("ffmpeg loudnorm JSON was truncated for {}", input.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&json_text[..json_end])
            .context("Failed to parse ffmpeg loudnorm JSON")?;

        let get = |key: &str| -> Result<String> {
            parsed
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| anyhow!("ffmpeg loudnorm JSON missing field '{}'", key))
        };
        let measured_i = get("input_i")?;
        let measured_tp = get("input_tp")?;
        let measured_lra = get("input_lra")?;
        let measured_thresh = get("input_thresh")?;
        let target_offset = get("target_offset").unwrap_or_else(|_| "0.0".to_string());

        // Pass 2: apply the static gain (linear=true disables dynamic processing)
        let loudnorm_filter = format!(
            "loudnorm=I={}:TP={}:LRA={}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
            LOUDNORM_TARGET_LUFS,
            LOUDNORM_TRUE_PEAK,
            LOUDNORM_LRA,
            measured_i,
            measured_tp,
            measured_lra,
            measured_thresh,
            target_offset,
        );

        let status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-nostdin",
                "-y",
                "-i",
                &input.to_string_lossy(),
                "-af",
                &loudnorm_filter,
                "-ar",
                "48000",
                "-ac",
                "1",
                &output.to_string_lossy(),
            ])
            .status()
            .context("Failed to run ffmpeg loudness normalization")?;

        if !status.success() {
            anyhow::bail!("Loudness normalization failed for {}", input.display());
        }

        emit(
            Level::Success,
            "video.enhance.loudnorm",
            &format!("Loudness normalization complete: {}", output.display()),
            None,
        );

        Ok(())
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
        let enhanced_cache_path = cache.path("enhanced.wav");

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
            extract_audio_to_wav(input, &wav_path)?;
        }

        // Step 2: Run DeepFilterNet
        let denoised_path = Self::run_deepfilter(&wav_path, &cache.cache_dir)?;

        // Step 3: Run static loudness normalization (no compression)
        Self::run_loudnorm(&denoised_path, &enhanced_cache_path)?;

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
