//! Core types for audio preprocessing

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Type of audio preprocessor to use
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PreprocessorType {
    /// Local processing using DeepFilterNet + ffmpeg-normalize
    #[default]
    Local,
    /// Cloud processing via Auphonic API
    Auphonic,
    /// Skip preprocessing entirely
    None,
}

impl std::fmt::Display for PreprocessorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreprocessorType::Local => write!(f, "Local (DeepFilterNet + ffmpeg-normalize)"),
            PreprocessorType::Auphonic => write!(f, "Auphonic (cloud processing)"),
            PreprocessorType::None => write!(f, "None (skip preprocessing)"),
        }
    }
}

/// Result from audio preprocessing
pub struct PreprocessResult {
    /// Path to the processed audio file
    pub output_path: PathBuf,
}

/// Trait for audio preprocessing backends
#[async_trait]
pub trait AudioPreprocessor: Send + Sync {
    /// Process an audio/video file and return path to processed audio
    ///
    /// # Arguments
    /// * `input` - Path to input audio or video file
    /// * `force` - Force reprocessing even if cached result exists
    async fn process(&self, input: &Path, force: bool) -> Result<PreprocessResult>;

    /// Human-readable name of the preprocessor for logging
    fn name(&self) -> &'static str;

    /// Check if the preprocessor's dependencies are available
    fn is_available(&self) -> bool;
}
