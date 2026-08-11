//! Audio enhancement module with pluggable backends
//!
//! Enhancement is the finishing stage for the *rendered* video's sound:
//! it denoises and levels the voice stem before the music bed is mixed in.
//! Supports multiple enhancement backends:
//! - `ClearVoice`: Alibaba DAMO AI speech enhancement (default)
//! - `Local`: DeepFilterNet noise reduction + EBU R128 dynamic loudness (voice only)
//! - `Auphonic`: Cloud-based processing via Auphonic API
//! - `None`: Skip enhancement

pub mod auphonic;
pub mod clearvoice;
pub mod local;
mod types;

use anyhow::{Context, Result};
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

/// Podcast-style mastering applied after speech enhancement.
///
/// ClearVoice/DeepFilterNet handle restoration; this stage supplies deliberate
/// tonal shaping, RMS compression, and final loudness/true-peak control. It is
/// intentionally restrained so different microphones remain natural:
/// - a small 250 Hz cut reduces proximity/mud;
/// - a small 3.2 kHz boost improves speech intelligibility;
/// - a short, boundary-safe dynamic stage evens out mic distance;
/// - soft-knee compression controls peaks;
/// - fixed gain plus a limiter produces stable streaming-video loudness.
///
/// This runs on the mono voice stem before music is mixed, so mastered music is
/// never EQed or compressed a second time.
pub(crate) const VOICE_MASTERING_FILTER: &str = concat!(
    "equalizer=f=250:t=q:w=0.9:g=-1.2,",
    "equalizer=f=3200:t=q:w=0.9:g=1.4,",
    // FFmpeg's default dynamic/loudnorm smoothing caused a roughly 20-second
    // startup fade. A five-frame (1.25 s) window reacts to mic distance while
    // alternative boundary handling gives the beginning and end a full gain
    // window instead of slowly ramping it in.
    "dynaudnorm=f=250:g=5:p=0.891:m=15:r=0.12:b=1:c=1:t=0.01,",
    "acompressor=threshold=0.10:ratio=2.5:attack=10:release=140:",
    "knee=4:detection=rms:makeup=1.5,",
    "volume=9dB,",
    // Leave enough sample-peak headroom that reconstructed true peak remains
    // at approximately -1 dBTP after resampling/encoding.
    "alimiter=limit=0.708:level=false:latency=true"
);

pub(crate) const VOICE_MASTERING_RECIPE: &str = concat!(
    "podcast-master:v2;eq:250Hz=-1.2dB:q0.9:3200Hz=+1.4dB:q0.9;",
    "distance-leveler:250ms:5frames:alt-boundary:maxgain=15:target-rms=0.12;",
    "compressor:rms:threshold=-20dB:ratio=2.5:1:attack=10ms:",
    "release=140ms:knee=4:makeup=1.5;gain=9dB;limit=0.708;48kHz-mono"
);

pub(crate) fn run_voice_mastering(input: &Path, output: &Path) -> Result<()> {
    emit(
        Level::Info,
        "video.enhance.mastering",
        "Mastering voice (podcast EQ, distance leveling, compression, and limiting)...",
        None,
    );

    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-af",
            VOICE_MASTERING_FILTER,
            "-ar",
            "48000",
            "-ac",
            "1",
            &output.to_string_lossy(),
        ])
        .status()
        .context("Failed to run ffmpeg voice mastering")?;

    if !status.success() {
        anyhow::bail!("ffmpeg voice mastering failed for {}", input.display());
    }

    emit(
        Level::Success,
        "video.enhance.mastering",
        &format!("Voice mastering complete: {}", output.display()),
        None,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mastering_uses_short_boundary_safe_leveling() {
        assert!(VOICE_MASTERING_FILTER.contains("dynaudnorm=f=250:g=5"));
        assert!(VOICE_MASTERING_FILTER.contains(":b=1:"));
        assert!(!VOICE_MASTERING_FILTER.contains("loudnorm"));
        assert!(!VOICE_MASTERING_FILTER.contains("gausssize=31"));
    }

    #[test]
    fn mastering_recipe_versions_cached_outputs() {
        assert!(VOICE_MASTERING_RECIPE.contains("podcast-master:v2"));
    }
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
