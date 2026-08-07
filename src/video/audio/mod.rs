//! Audio enhancement module with pluggable backends
//!
//! Enhancement is the finishing stage for the *rendered* video's sound:
//! it denoises and levels the voice stem before the music bed is mixed in.
//! Supports multiple enhancement backends:
//! - `ClearVoice`: Alibaba DAMO AI speech enhancement (default)
//! - `Local`: DeepFilterNet noise reduction + pure EBU R128 loudness (no compression)
//! - `Auphonic`: Cloud-based processing via Auphonic API
//! - `None`: Skip enhancement

pub mod auphonic;
pub mod clearvoice;
pub mod local;
mod types;

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

pub use types::{AudioEnhancer, EnhanceResult, EnhancerType};

use crate::ui::prelude::{Level, emit};
use crate::video::config::VideoDirectories;
use crate::video::support::utils::compute_file_hash;
use crate::video::transcript_language::TranscriptLanguage;

use super::config::VideoConfig;

/// Create an enhancer instance based on type
pub fn create_enhancer(
    enhancer_type: &EnhancerType,
    config: &VideoConfig,
) -> Box<dyn AudioEnhancer> {
    match enhancer_type {
        EnhancerType::ClearVoice => Box::new(clearvoice::ClearVoiceEnhancer::new()),
        EnhancerType::Local => Box::new(local::LocalEnhancer::new()),
        EnhancerType::Auphonic => Box::new(auphonic::AuphonicEnhancer::new(
            config.auphonic_api_key.clone(),
            config.auphonic_preset_uuid.clone(),
        )),
        EnhancerType::None => Box::new(NoneEnhancer),
    }
}

/// Parse enhancer type from string
pub fn parse_enhancer_type(s: &str) -> Result<EnhancerType> {
    match s.to_lowercase().as_str() {
        "clearvoice" | "clear" | "cv" => Ok(EnhancerType::ClearVoice),
        "local" => Ok(EnhancerType::Local),
        "auphonic" => Ok(EnhancerType::Auphonic),
        "none" => Ok(EnhancerType::None),
        _ => anyhow::bail!(
            "Unknown enhancer type: '{}'. Expected: clearvoice, local, auphonic, or none",
            s
        ),
    }
}

/// Shared cache setup for audio enhancers.
///
/// Both backends hash the input, resolve the shared cache directory and check
/// for an already-enhanced result before running their own pipeline, so this
/// keeps that boilerplate in one place.
pub(crate) struct EnhanceCache {
    pub(crate) input_hash: String,
    pub(crate) cache_dir: PathBuf,
}

impl EnhanceCache {
    /// Hashes the input and prepares the shared processing cache directory.
    pub(crate) fn prepare(input: &Path) -> Result<Self> {
        let input_hash = compute_file_hash(input)?;
        let directories = VideoDirectories::new()?;
        let cache_paths = directories.cache_paths(&input_hash, TranscriptLanguage::En);
        cache_paths.ensure_directories()?;
        Ok(Self {
            input_hash,
            cache_dir: cache_paths.transcript_dir().to_path_buf(),
        })
    }

    /// Cache path for a `<hash>_<suffix>` file inside the cache directory.
    pub(crate) fn path(&self, suffix: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}_{}", self.input_hash, suffix))
    }

    /// Returns the cached result when present (unless `force` reprocessing is
    /// requested), logging reuse of the cache under the given event name.
    pub(crate) fn cached(
        &self,
        enhanced_path: &Path,
        force: bool,
        event: &str,
        backend: &str,
    ) -> Option<EnhanceResult> {
        if !enhanced_path.exists() || force {
            return None;
        }
        let backend_prefix = if backend.is_empty() {
            String::new()
        } else {
            format!("{backend} ")
        };
        emit(
            Level::Info,
            event,
            &format!(
                "Using cached {backend_prefix}enhanced audio: {}",
                enhanced_path.display()
            ),
            None,
        );
        Some(EnhanceResult {
            output_path: enhanced_path.to_path_buf(),
        })
    }
}

/// Pure static 2-pass EBU R128 loudness normalization (no compression).
///
/// Pass 1 measures the true integrated loudness, pass 2 applies the measured
/// values with linear normalization so the result lands at `-14 LUFS` with a
/// `-1.0 dBTP` ceiling. Shared by the ClearVoice and Local backends; the
/// enhancer (AI model) does the denoising, this only levels the voice stem.
/// Pure static 2-pass EBU R128 loudness normalization (no compression).
///
/// Pass 1 measures the true integrated loudness; pass 2 applies the measured
/// values with `linear=true`, so ffmpeg rescales by a constant factor instead
/// of running its dynamic compressor/limiter. The voice keeps its natural
/// dynamics; only overall level moves toward the target (-14 LUFS, -1.0 dBTP).
pub(crate) fn run_static_loudnorm(input: &Path, output: &Path) -> Result<()> {
    use std::process::Command;

    let target_i = "-14";
    let target_tp = "-1.0";
    let target_lra = "11";

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
                target_i, target_tp, target_lra
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
        target_i,
        target_tp,
        target_lra,
        measured_i,
        measured_tp,
        measured_lra,
        measured_thresh,
        target_offset
    );
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-af",
            &loudnorm_filter,
            "-ar",
            "48000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
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

/// No-op enhancer that returns input unchanged
struct NoneEnhancer;

#[async_trait::async_trait]
impl AudioEnhancer for NoneEnhancer {
    async fn enhance(&self, input: &Path, _force: bool) -> Result<EnhanceResult> {
        Ok(EnhanceResult {
            output_path: input.to_path_buf(),
        })
    }

    fn name(&self) -> &'static str {
        "none"
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// Handle the `ins video enhance` CLI command
pub async fn handle_enhance(args: super::cli::EnhanceArgs) -> Result<()> {
    use crate::video::support::utils::canonicalize_existing;

    let input_path = canonicalize_existing(&args.input_file)?;
    let config = VideoConfig::load()?;

    // "auto" resolves to the configured enhancer (default: ClearVoice)
    let enhancer_type = if args.backend.eq_ignore_ascii_case("auto") {
        config.enhancer.clone()
    } else {
        parse_enhancer_type(&args.backend)?
    };

    let enhancer: Box<dyn AudioEnhancer> = match enhancer_type {
        EnhancerType::Auphonic => Box::new(auphonic::AuphonicEnhancer::new(
            args.api_key.or(config.auphonic_api_key),
            args.preset.or(config.auphonic_preset_uuid),
        )),
        _ => create_enhancer(&enhancer_type, &config),
    };

    if !enhancer.is_available() {
        anyhow::bail!(
            "Enhancer '{}' is not available. Check that required tools are installed.",
            enhancer.name()
        );
    }

    emit(
        Level::Info,
        "video.enhance.start",
        &format!(
            "Enhancing {} with {} backend...",
            input_path.display(),
            enhancer.name()
        ),
        None,
    );

    let result = enhancer.enhance(&input_path, args.force).await?;

    // Copy to output location next to input, preserving the enhanced file's extension
    let output_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let input_stem = input_path.file_stem().unwrap_or_default();
    let output_ext = result
        .output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");
    let output_filename = format!("{}_enhanced.{}", input_stem.to_string_lossy(), output_ext);
    let output_path = output_dir.join(output_filename);

    if result.output_path != output_path {
        std::fs::copy(&result.output_path, &output_path)?;
    }

    emit(
        Level::Success,
        "video.enhance.success",
        &format!("Saved enhanced audio to {}", output_path.display()),
        None,
    );

    Ok(())
}
