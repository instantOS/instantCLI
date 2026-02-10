use std::path::Path;

use crate::video::render::timeline::{Segment, SegmentData, Timeline};

pub fn categorize_segments(
    timeline: &Timeline,
) -> (Vec<&Segment>, Vec<&Segment>, Vec<&Segment>, Vec<&Segment>) {
    let mut video = Vec::new();
    let mut overlay = Vec::new();
    let mut music = Vec::new();
    let mut broll = Vec::new();

    for segment in &timeline.segments {
        match &segment.data {
            SegmentData::VideoSubset { .. } => video.push(segment),
            SegmentData::Image { .. } => overlay.push(segment),
            SegmentData::Music { .. } => music.push(segment),
            SegmentData::Broll { .. } => broll.push(segment),
        }
    }
    (video, overlay, music, broll)
}

pub fn format_time(value: f64) -> String {
    format!("{value:.6}")
}

pub fn escape_ffmpeg_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('\'', "'\\''")
        .replace(':', "\\:")
}
