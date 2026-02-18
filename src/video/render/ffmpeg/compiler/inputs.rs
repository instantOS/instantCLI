use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::video::render::timeline::Timeline;

/// Registry mapping media file paths to FFmpeg input indices.
///
/// When FFmpeg processes multiple input files, each is assigned a zero-based
/// index based on the order of `-i` arguments. This struct provides a clean
/// API for registering sources and looking up their indices.
pub struct SourceMap {
    map: HashMap<PathBuf, usize>,
    order: Vec<PathBuf>,
}

impl SourceMap {
    /// Build a SourceMap from a timeline and global audio source.
    ///
    /// Collects every media file referenced by the timeline and assigns each
    /// a unique FFmpeg input index. Audio paths embedded in segments are
    /// included automatically.
    pub fn build(timeline: &Timeline, audio_source: &Path) -> Self {
        let mut map: HashMap<PathBuf, usize> = HashMap::new();
        let mut order: Vec<PathBuf> = Vec::new();
        let mut next_index = 0;

        for segment in &timeline.segments {
            if let Some(source) = segment.data.source_path()
                && !map.contains_key(source)
            {
                map.insert(source.clone(), next_index);
                order.push(source.clone());
                next_index += 1;
            }
            if let Some(audio) = segment.data.audio_source()
                && !map.contains_key(audio)
            {
                map.insert(audio.clone(), next_index);
                order.push(audio.clone());
                next_index += 1;
            }
        }

        if !map.contains_key(audio_source) {
            map.insert(audio_source.to_path_buf(), next_index);
            order.push(audio_source.to_path_buf());
        }

        Self { map, order }
    }

    /// Get the FFmpeg input index for a source file.
    pub fn index(&self, path: &Path) -> Result<usize> {
        self.map
            .get(path)
            .copied()
            .ok_or_else(|| anyhow!("No FFmpeg input available for {}", path.display()))
    }

    /// Get the FFmpeg input index with custom context in error message.
    ///
    /// Example: `source_map.index_for(path, "B-roll video")` produces:
    /// "No FFmpeg input available for B-roll video: /path/to/file.mp4"
    pub fn index_for(&self, path: &Path, context: &str) -> Result<usize> {
        self.map.get(path).copied().ok_or_else(|| {
            anyhow!(
                "No FFmpeg input available for {}: {}",
                context,
                path.display()
            )
        })
    }

    /// Source paths in FFmpeg input order.
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.order.iter()
    }

    /// Generate `-i` argument pairs for FFmpeg command.
    ///
    /// Returns a flat vector: `["-i", "/path/1.mp4", "-i", "/path/2.mp4", ...]`
    pub fn input_args(&self) -> Vec<String> {
        let mut args = Vec::with_capacity(self.order.len() * 2);
        for source in &self.order {
            args.push("-i".to_string());
            args.push(source.to_string_lossy().into_owned());
        }
        args
    }
}
