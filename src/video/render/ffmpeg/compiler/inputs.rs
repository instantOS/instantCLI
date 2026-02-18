use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::video::render::timeline::Timeline;

use super::FfmpegCompiler;

impl FfmpegCompiler {
    /// Collect every media file referenced by the timeline and assign each a
    /// unique ffmpeg input index (`-i` position).
    ///
    /// Returns:
    /// - `source_map`: file path → ffmpeg input index
    /// - `source_order`: paths in the order they will appear as `-i` arguments
    ///
    /// Sources are added in this priority order:
    /// 1. Video/audio files referenced directly by timeline segments
    /// 2. Per-source audio tracks from `audio_by_source_id`
    /// 3. The primary `audio_source` (default/fallback audio track)
    pub(super) fn build_input_source_map(
        &self,
        timeline: &Timeline,
        audio_source: &Path,
        audio_by_source_id: &HashMap<String, PathBuf>,
    ) -> (HashMap<PathBuf, usize>, Vec<PathBuf>) {
        let mut source_map: HashMap<PathBuf, usize> = HashMap::new();
        let mut source_order: Vec<PathBuf> = Vec::new();
        let mut next_index = 0;

        for segment in &timeline.segments {
            if let Some(source) = segment.data.source_path()
                && !source_map.contains_key(source)
            {
                source_map.insert(source.clone(), next_index);
                source_order.push(source.clone());
                next_index += 1;
            }
            if let Some(audio) = segment.data.audio_source()
                && !source_map.contains_key(audio)
            {
                source_map.insert(audio.clone(), next_index);
                source_order.push(audio.clone());
                next_index += 1;
            }
        }

        for audio in audio_by_source_id.values() {
            if !source_map.contains_key(audio) {
                source_map.insert(audio.clone(), next_index);
                source_order.push(audio.clone());
                next_index += 1;
            }
        }

        if !source_map.contains_key(audio_source) {
            source_map.insert(audio_source.to_path_buf(), next_index);
            source_order.push(audio_source.to_path_buf());
        }

        (source_map, source_order)
    }
}

/// Look up the ffmpeg input index for a media file, returning a contextual
/// error if the file was not registered in the source map.
pub fn get_ffmpeg_input_index(
    source_map: &HashMap<PathBuf, usize>,
    source: &Path,
    error_prefix: &str,
) -> Result<usize> {
    source_map
        .get(source)
        .copied()
        .ok_or_else(|| anyhow!("{error_prefix} {}", source.display()))
}
