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

/// Conservative podcast-style mastering applied after speech enhancement.
///
/// ClearVoice/DeepFilterNet handle restoration; this stage supplies deliberate
/// tonal shaping, RMS compression, and final loudness/true-peak control. It is
/// intentionally restrained so different microphones remain natural:
/// - a small 250 Hz cut reduces proximity/mud;
/// - a small 3.2 kHz boost improves speech intelligibility;
/// - soft-knee 3:1 RMS compression evens out mic distance and delivery;
/// - EBU normalization targets streaming-video loudness at -14 LUFS/-1 dBTP.
///
/// This runs on the mono voice stem before music is mixed, so mastered music is
/// never EQed or compressed a second time.
pub(crate) const VOICE_MASTERING_FILTER: &str = concat!(
    "equalizer=f=250:t=q:w=0.9:g=-1.2,",
    "equalizer=f=3200:t=q:w=0.9:g=1.4,",
    "acompressor=threshold=0.02:ratio=3:attack=10:release=140:",
    "knee=4:detection=rms"
);

pub(crate) const VOICE_MASTERING_RECIPE: &str = concat!(
    "podcast-eq:v1:250Hz=-1.2dB:q0.9:3200Hz=+1.4dB:q0.9;",
    "compressor:rms:threshold=-34dB:ratio=3:1:attack=10ms:",
    "release=140ms:knee=4;ebu:target=-14LUFS:lra=4:true-peak=-1dBTP:48kHz"
);

pub(crate) fn run_loudnorm(input: &Path, output: &Path) -> Result<()> {
    emit(
        Level::Info,
        "video.enhance.loudnorm",
        "Mastering voice (podcast EQ, compression, and -14 LUFS normalization)...",
        None,
    );

    let status = std::process::Command::new("uvx")
        .args([
            "ffmpeg-normalize",
            &input.to_string_lossy(),
            "--preset",
            "streaming-video",
            "--dynamic",
            "--pre-filter",
            VOICE_MASTERING_FILTER,
            "-lrt",
            "4",
            "--true-peak",
            "-1.0",
            "--sample-rate",
            "48000",
            "-o",
            &output.to_string_lossy(),
            "-f",
        ])
        .status()
        .context("Failed to run ffmpeg-normalize")?;

    if !status.success() {
        anyhow::bail!("ffmpeg-normalize failed for {}", input.display());
    }

    emit(
        Level::Success,
        "video.enhance.loudnorm",
        &format!("Voice mastering complete: {}", output.display()),
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
