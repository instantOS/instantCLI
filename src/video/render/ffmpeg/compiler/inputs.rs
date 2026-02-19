use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::video::render::timeline::Timeline;

/// Registry mapping media file paths to FFmpeg input indices.
///
/// When FFmpeg processes multiple input files, each is assigned a zero-based
/// index based on the order of `-i` arguments. This struct provides a clean
/// API for registering sources and looking up their indices.
///
/// When `input_seeking` is enabled (for preview), the map also tracks the
/// earliest source timestamp per input. `-ss` is emitted before each `-i`
/// so FFmpeg can keyframe-seek, and `offset()` returns the value that
/// `trim` filters must subtract from their timestamps.
pub struct SourceMap {
    map: HashMap<PathBuf, usize>,
    order: Vec<PathBuf>,
    /// Per-input seek offsets. Empty when input seeking is disabled.
    offsets: Vec<f64>,
    input_seeking: bool,
}

impl SourceMap {
    /// Build a SourceMap from a timeline and global audio source.
    ///
    /// When `input_seeking` is false (render path), no `-ss` is emitted and
    /// `offset()` always returns 0. When true (preview path), the earliest
    /// source timestamp per input is tracked; `-ss` is emitted before `-i`
    /// and trim filters must subtract `offset()` from their timestamps.
    pub fn build(timeline: &Timeline, audio_source: &Path, input_seeking: bool) -> Self {
        let mut map: HashMap<PathBuf, usize> = HashMap::new();
        let mut order: Vec<PathBuf> = Vec::new();
        let mut offsets: Vec<f64> = Vec::new();
        let mut next_index = 0;

        for segment in &timeline.segments {
            if let Some(source) = segment.data.source_path() {
                let source_start = if input_seeking {
                    segment.data.source_start_time().unwrap_or(0.0)
                } else {
                    0.0
                };
                if let Some(&idx) = map.get(source) {
                    if input_seeking {
                        offsets[idx] = offsets[idx].min(source_start);
                    }
                } else {
                    map.insert(source.clone(), next_index);
                    order.push(source.clone());
                    offsets.push(source_start);
                    next_index += 1;
                }
            }
            if let Some(audio) = segment.data.audio_source() {
                let source_start = if input_seeking {
                    segment.data.source_start_time().unwrap_or(0.0)
                } else {
                    0.0
                };
                if let Some(&idx) = map.get(audio) {
                    if input_seeking {
                        offsets[idx] = offsets[idx].min(source_start);
                    }
                } else {
                    map.insert(audio.clone(), next_index);
                    order.push(audio.clone());
                    offsets.push(source_start);
                    next_index += 1;
                }
            }
        }

        if !map.contains_key(audio_source) {
            map.insert(audio_source.to_path_buf(), next_index);
            order.push(audio_source.to_path_buf());
            offsets.push(0.0);
        }

        Self {
            map,
            order,
            offsets,
            input_seeking,
        }
    }

    /// Get the FFmpeg input index for a source file.
    pub fn index(&self, path: &Path) -> Result<usize> {
        self.map
            .get(path)
            .copied()
            .ok_or_else(|| anyhow!("No FFmpeg input available for {}", path.display()))
    }

    /// Get the `-ss` seek offset applied to this input.
    /// Trim filters should subtract this from their start/end times.
    /// Returns 0 when input seeking is disabled (render path).
    pub fn offset(&self, input_index: usize) -> f64 {
        if !self.input_seeking {
            return 0.0;
        }
        self.offsets.get(input_index).copied().unwrap_or(0.0)
    }

    /// Source paths in FFmpeg input order.
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.order.iter()
    }

    /// Generate input argument pairs for FFmpeg command.
    ///
    /// When input seeking is enabled, emits `-ss <time>` before each `-i`
    /// so FFmpeg can keyframe-seek to the earliest referenced point.
    /// When disabled, emits plain `-i` arguments.
    pub fn input_args(&self) -> Vec<String> {
        let mut args = Vec::with_capacity(self.order.len() * 4);
        for (i, source) in self.order.iter().enumerate() {
            if self.input_seeking {
                let offset = self.offsets[i];
                if offset > 0.0 {
                    args.push("-ss".to_string());
                    args.push(format!("{:.6}", offset));
                }
            }
            args.push("-i".to_string());
            args.push(source.to_string_lossy().into_owned());
        }
        args
    }
}
