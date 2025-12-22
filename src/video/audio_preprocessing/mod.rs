//! Audio preprocessing module with pluggable backends
//!
//! Supports multiple audio preprocessing backends:
//! - `Local`: Uses DeepFilterNet for noise reduction + ffmpeg-normalize for loudness
//! - `Auphonic`: Cloud-based processing via Auphonic API
//! - `None`: Skip preprocessing

pub mod auphonic;
pub mod local;
mod types;

use anyhow::Result;
use std::path::Path;

pub use types::{AudioPreprocessor, PreprocessResult, PreprocessorType};

use crate::ui::prelude::{Level, emit};

use super::config::VideoConfig;

/// Create a preprocessor instance based on type
pub fn create_preprocessor(
    preprocessor_type: &PreprocessorType,
    config: &VideoConfig,
) -> Box<dyn AudioPreprocessor> {
    match preprocessor_type {
        PreprocessorType::Local => Box::new(local::LocalPreprocessor::new()),
        PreprocessorType::Auphonic => Box::new(auphonic::AuphonicPreprocessor::new(
            config.auphonic_api_key.clone(),
            config.auphonic_preset_uuid.clone(),
        )),
        PreprocessorType::None => Box::new(NonePreprocessor),
    }
}

/// Parse preprocessor type from string
pub fn parse_preprocessor_type(s: &str) -> Result<PreprocessorType> {
    match s.to_lowercase().as_str() {
        "local" => Ok(PreprocessorType::Local),
        "auphonic" => Ok(PreprocessorType::Auphonic),
        "none" => Ok(PreprocessorType::None),
        _ => anyhow::bail!(
            "Unknown preprocessor type: '{}'. Expected: local, auphonic, or none",
            s
        ),
    }
}

/// No-op preprocessor that returns input unchanged
struct NonePreprocessor;

#[async_trait::async_trait]
impl AudioPreprocessor for NonePreprocessor {
    async fn process(&self, input: &Path, _force: bool) -> Result<PreprocessResult> {
        Ok(PreprocessResult {
            output_path: input.to_path_buf(),
            cached: true,
        })
    }

    fn name(&self) -> &'static str {
        "none"
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// Handle the preprocess CLI command
pub async fn handle_preprocess(args: super::cli::PreprocessArgs) -> Result<()> {
    use super::utils::canonicalize_existing;

    let input_path = canonicalize_existing(&args.input_file)?;
    let preprocessor_type = parse_preprocessor_type(&args.backend)?;

    let config = VideoConfig::load()?;

    let preprocessor: Box<dyn AudioPreprocessor> = match preprocessor_type {
        PreprocessorType::Auphonic => Box::new(auphonic::AuphonicPreprocessor::new(
            args.api_key.or(config.auphonic_api_key),
            args.preset.or(config.auphonic_preset_uuid),
        )),
        _ => create_preprocessor(&preprocessor_type, &config),
    };

    if !preprocessor.is_available() {
        anyhow::bail!(
            "Preprocessor '{}' is not available. Check that required tools are installed.",
            preprocessor.name()
        );
    }

    emit(
        Level::Info,
        "video.preprocess.start",
        &format!(
            "Processing {} with {} backend...",
            input_path.display(),
            preprocessor.name()
        ),
        None,
    );

    let result = preprocessor.process(&input_path, args.force).await?;

    // Copy to output location next to input
    let output_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let input_stem = input_path.file_stem().unwrap_or_default();
    let output_filename = format!("{}_processed.mp3", input_stem.to_string_lossy());
    let output_path = output_dir.join(output_filename);

    if result.output_path != output_path {
        std::fs::copy(&result.output_path, &output_path)?;
    }

    emit(
        Level::Success,
        "video.preprocess.success",
        &format!("Saved processed file to {}", output_path.display()),
        None,
    );

    Ok(())
}
