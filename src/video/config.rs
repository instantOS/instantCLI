use anyhow::{Context, Result};
use dirs::{cache_dir, data_dir};
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
