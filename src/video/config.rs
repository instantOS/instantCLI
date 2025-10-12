use anyhow::{Context, Result};
use dirs::{cache_dir, config_dir, data_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub struct VideoDirectories {
    data_root: PathBuf,
    cache_root: PathBuf,
}

impl VideoDirectories {
    pub fn new() -> Result<Self> {
        let data_root = data_dir()
            .context("Unable to determine data directory for video projects")?
            .join("instant")
            .join("video");

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

    pub fn project_paths(&self, video_hash: &str) -> VideoProjectPaths {
        let project_dir = self.data_root.join(video_hash);
        let transcript_dir = self.cache_root.join(video_hash);
        VideoProjectPaths {
            video_hash: video_hash.to_string(),
            project_dir,
            transcript_dir,
            markdown_path: PathBuf::from("video.md"),
            metadata_path: PathBuf::from("metadata.yaml"),
            transcript_cache_file: PathBuf::from("transcript.srt"),
        }
        .resolve()
    }
}

pub struct VideoProjectPaths {
    video_hash: String,
    project_dir: PathBuf,
    transcript_dir: PathBuf,
    markdown_path: PathBuf,
    metadata_path: PathBuf,
    transcript_cache_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    pub music_volume: f32,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            music_volume: Self::DEFAULT_MUSIC_VOLUME,
        }
    }
}

impl VideoConfig {
    pub const DEFAULT_MUSIC_VOLUME: f32 = 0.2;

    pub fn load() -> Result<Self> {
        Self::load_from_path(video_config_path()?)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            let config = Self::default();
            config.save_to_path(path)?;
            return Ok(config);
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading video config from {}", path.display()))?;
        let mut config: Self = toml::from_str(&contents).context("parsing video config")?;
        if !config.music_volume.is_finite() || config.music_volume < 0.0 {
            config.music_volume = Self::DEFAULT_MUSIC_VOLUME;
        }
        Ok(config)
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

fn video_config_path() -> Result<PathBuf> {
    let config_root = config_dir()
        .context("Unable to determine config directory")?
        .join("instant");
    Ok(config_root.join("video.toml"))
}

impl VideoProjectPaths {
    fn resolve(mut self) -> Self {
        self.markdown_path = self.project_dir.join(self.markdown_path);
        self.metadata_path = self.project_dir.join(self.metadata_path);
        self.transcript_cache_file = self.transcript_dir.join(format!("{}.srt", self.video_hash));
        self
    }

    pub fn ensure_directories(&self) -> Result<()> {
        fs::create_dir_all(&self.project_dir).with_context(|| {
            format!(
                "Failed to create project directory {}",
                self.project_dir.display()
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

    pub fn markdown_path(&self) -> &Path {
        &self.markdown_path
    }

    pub fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub fn transcript_cache_path(&self) -> &Path {
        &self.transcript_cache_file
    }

    pub fn hashed_video_input(&self, extension: &str) -> PathBuf {
        self.transcript_dir
            .join(format!("{}.{}", self.video_hash, extension))
    }
}
