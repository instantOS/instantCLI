use anyhow::{Context, Result};
use dirs::cache_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// Import macro from crate root (#[macro_export] places it there)
use crate::common::config::DocumentedConfig;
use crate::common::paths;
use crate::documented_config;

pub use super::audio::PreprocessorType;

/// Data directories for video projects.
///
/// - `data_root`: Stores project files (markdown, metadata)
/// - `cache_root`: Stores transient files (transcripts, processed audio)
pub struct VideoDirectories {
    data_root: PathBuf,
    cache_root: PathBuf,
}

impl VideoDirectories {
    pub fn new() -> Result<Self> {
        let data_root = paths::instant_video_dir()?;

        let cache_root = cache_dir()
            .context("Unable to determine cache directory for video projects")?
            .join("instant")
            .join("video");

        fs::create_dir_all(&data_root).with_context(|| {
            format!(
                "Failed to create video data directory at {}",
                data_root.display()
            )
        })?;
        fs::create_dir_all(&cache_root).with_context(|| {
            format!(
                "Failed to create video cache directory at {}",
                cache_root.display()
            )
        })?;

        Ok(Self {
            data_root,
            cache_root,
        })
    }

    pub fn cache_paths(&self, video_hash: &str) -> VideoCachePaths {
        let data_dir = self.data_root.join(video_hash);
        let transcript_dir = self.cache_root.join(video_hash);
        VideoCachePaths {
            video_hash: video_hash.to_string(),
            data_dir,
            transcript_dir,
            transcript_cache_path: PathBuf::new(),
        }
        .resolve()
    }
}

/// Paths for a single video project, keyed by video hash.
///
/// Contains all file paths needed for video processing:
/// - `data_dir/`: Hash-keyed data directory
/// - `transcript_dir/`: Contains cached transcripts and processed audio
pub struct VideoCachePaths {
    video_hash: String,
    data_dir: PathBuf,
    transcript_dir: PathBuf,
    transcript_cache_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    /// Music volume for video processing (0.0-1.0)
    #[serde(default = "crate::video::config::VideoConfig::default_music_volume")]
    pub music_volume: f32,
    /// Which audio preprocessor to use (local, auphonic, or none)
    pub preprocessor: PreprocessorType,
    /// Auphonic API key (only used when preprocessor = auphonic)
    pub auphonic_api_key: Option<String>,
    /// Auphonic preset UUID (only used when preprocessor = auphonic)
    pub auphonic_preset_uuid: Option<String>,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            music_volume: Self::DEFAULT_MUSIC_VOLUME,
            preprocessor: PreprocessorType::default(),
            auphonic_api_key: None,
            auphonic_preset_uuid: None,
        }
    }
}

impl VideoConfig {
    fn default_music_volume() -> f32 {
        0.1
    }

    pub const DEFAULT_MUSIC_VOLUME: f32 = 0.1;

    pub fn load() -> Result<Self> {
        <Self as DocumentedConfig>::load_from_path_documented(video_config_path()?)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(video_config_path()?)
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating video config directory {}", parent.display()))?;
        }

        let toml = toml::to_string_pretty(self).context("serializing video config")?;
        fs::write(path, toml)
            .with_context(|| format!("writing video config to {}", path.display()))?;
        Ok(())
    }

    pub fn music_volume(&self) -> f32 {
        if !self.music_volume.is_finite() || self.music_volume < 0.0 {
            Self::DEFAULT_MUSIC_VOLUME
        } else {
            self.music_volume
        }
    }
}

// Implement DocumentedConfig trait for VideoConfig using the macro
documented_config!(VideoConfig,
    music_volume, "Music volume for video processing (0.0-1.0)",
    preprocessor, "Which audio preprocessor to use (local, auphonic, or none)",
    auphonic_api_key, "Auphonic API key for cloud preprocessing",
    auphonic_preset_uuid, "Auphonic preset UUID for consistent processing settings",
    => Ok(paths::instant_config_dir()?.join("video.toml"))
);

fn video_config_path() -> Result<PathBuf> {
    Ok(paths::instant_config_dir()?.join("video.toml"))
}

impl VideoCachePaths {
    fn resolve(mut self) -> Self {
        self.transcript_cache_path = self
            .transcript_dir
            .join(format!("{}.json", self.video_hash));
        self
    }

    pub fn ensure_directories(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir).with_context(|| {
            format!(
                "Failed to create data directory {}",
                self.data_dir.display()
            )
        })?;
        fs::create_dir_all(&self.transcript_dir).with_context(|| {
            format!(
                "Failed to create transcript cache directory {}",
                self.transcript_dir.display()
            )
        })?;
        Ok(())
    }

    pub fn transcript_dir(&self) -> &Path {
        &self.transcript_dir
    }

    pub fn transcript_cache_path(&self) -> &Path {
        &self.transcript_cache_path
    }

    pub fn hashed_video_input(&self, extension: &str) -> PathBuf {
        self.transcript_dir
            .join(format!("{}.{}", self.video_hash, extension))
    }
}
