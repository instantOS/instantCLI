//! Core types for audio enhancement (the final-sound finishing stage:
//! denoise + loudness. Applied to the voice stem before music is mixed.)

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Type of audio enhancer to use
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EnhancerType {
    /// ClearVoice (Alibaba DAMO) AI speech enhancement (default)
    #[default]
    ClearVoice,
    /// Local enhancer using DeepFilterNet + pure EBU R128 loudness
    Local,
    /// Cloud enhancement via Auphonic API
    Auphonic,
    /// Skip enhancement entirely
    None,
}

impl std::fmt::Display for EnhancerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnhancerType::ClearVoice => write!(f, "ClearVoice (AI enhancement)"),
            EnhancerType::Local => write!(f, "Local (DeepFilterNet + loudness)"),
            EnhancerType::Auphonic => write!(f, "Auphonic (cloud processing)"),
            EnhancerType::None => write!(f, "None (no enhancement)"),
        }
    }
}

/// Result from audio enhancement
pub struct EnhanceResult {
    /// Path to the enhanced audio file
    pub output_path: PathBuf,
}

/// Trait for audio enhancement backends
#[async_trait]
pub trait AudioEnhancer: Send + Sync {
    /// Enhance an audio/video file and return path to the enhanced audio.
    ///
    /// # Arguments
    /// * `input` - Path to input audio or video file
    /// * `force` - Force reprocessing even if a cached result exists
    async fn enhance(&self, input: &Path, force: bool) -> Result<EnhanceResult>;

    /// Human-readable name of the enhancer for logging
    fn name(&self) -> &'static str;

    /// Check if the enhancer's dependencies are available
    fn is_available(&self) -> bool;
}
