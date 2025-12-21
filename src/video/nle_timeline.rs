use super::timeline::TimelinePlan;
use super::utils::compute_file_hash;
use crate::ui::prelude::{Level, emit};
use anyhow::Context;
use std::path::{Path, PathBuf};

/// Non-linear editor style timeline structure
/// This represents a sequence of segments that will be rendered in order
#[derive(Debug, Clone)]
pub struct Timeline {
    pub segments: Vec<Segment>,
}

/// A segment in the timeline with a start time, duration, and data
#[derive(Debug, Clone)]
pub struct Segment {
    /// Start time in the final rendered timeline (in seconds)
    pub start_time: f64,
    /// Duration of this segment (in seconds)
    pub duration: f64,
    /// The actual content/data of this segment
    pub data: SegmentData,
}

/// The different types of content that can appear in a timeline segment
#[derive(Debug, Clone)]
pub enum SegmentData {
    /// A subset of a source video with optional transform
    VideoSubset {
        /// Start time in the source video (in seconds)
        start_time: f64,
        /// Path to the source video file
        source_video: PathBuf,
        /// Optional transform to apply to this video segment
        transform: Option<Transform>,
    },
    /// A static image with optional transform
    Image {
        /// Path to the source image file
        source_image: PathBuf,
        /// Optional transform to apply to this image
        transform: Option<Transform>,
    },
    /// An audio/music source
    Music {
        /// Path to the audio source file
        audio_source: PathBuf,
    },
}

/// Transform operations that can be applied to video or image segments
#[derive(Debug, Clone)]
pub struct Transform {
    /// Scale factor (e.g., 1.0 = 100%, 0.5 = 50%, 2.0 = 200%)
    pub scale: Option<f32>,
    /// Rotation in degrees
    pub rotate: Option<f32>,
    /// Translation (x, y) in pixels
    pub translate: Option<(f32, f32)>,
}

impl Timeline {
    /// Create a new empty timeline
    pub fn new() -> Self {
        Timeline {
            segments: Vec::new(),
        }
    }

    /// Add a segment to the timeline
    pub fn add_segment(&mut self, segment: Segment) {
        self.segments.push(segment);
    }

    /// Get the total duration of the timeline
    pub fn total_duration(&self) -> f64 {
        self.segments
            .iter()
            .map(|s| s.start_time + s.duration)
            .fold(0.0, f64::max)
    }

    /// Get all segments that overlap with a given time range
    pub fn segments_in_range(&self, start: f64, end: f64) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| {
                let seg_end = s.start_time + s.duration;
                // Check if segments overlap
                s.start_time < end && seg_end > start
            })
            .collect()
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Segment {
    /// Create a new video subset segment
    pub fn new_video_subset(
        start_time: f64,
        duration: f64,
        source_start: f64,
        source_video: PathBuf,
        transform: Option<Transform>,
    ) -> Self {
        Segment {
            start_time,
            duration,
            data: SegmentData::VideoSubset {
                start_time: source_start,
                source_video,
                transform,
            },
        }
    }

    /// Create a new image segment
    pub fn new_image(
        start_time: f64,
        duration: f64,
        source_image: PathBuf,
        transform: Option<Transform>,
    ) -> Self {
        Segment {
            start_time,
            duration,
            data: SegmentData::Image {
                source_image,
                transform,
            },
        }
    }

    /// Create a new music segment
    pub fn new_music(start_time: f64, duration: f64, audio_source: PathBuf) -> Self {
        Segment {
            start_time,
            duration,
            data: SegmentData::Music { audio_source },
        }
    }

    /// Get the end time of this segment
    pub fn end_time(&self) -> f64 {
        self.start_time + self.duration
    }
}

impl Transform {
    /// Create a new transform with no operations
    pub fn new() -> Self {
        Transform {
            scale: None,
            rotate: None,
            translate: None,
        }
    }

    /// Create a transform with only scale
    pub fn with_scale(scale: f32) -> Self {
        Transform {
            scale: Some(scale),
            rotate: None,
            translate: None,
        }
    }

    /// Create a transform with only rotation
    pub fn with_rotation(degrees: f32) -> Self {
        Transform {
            scale: None,
            rotate: Some(degrees),
            translate: None,
        }
    }

    /// Create a transform with only translation
    pub fn with_translation(x: f32, y: f32) -> Self {
        Transform {
            scale: None,
            rotate: None,
            translate: Some((x, y)),
        }
    }

    /// Check if this transform has any operations
    pub fn is_identity(&self) -> bool {
        self.scale.is_none() && self.rotate.is_none() && self.translate.is_none()
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::new()
    }
}

impl SegmentData {
    /// Get the source path for this segment data (if applicable)
    pub fn source_path(&self) -> Option<&PathBuf> {
        match self {
            SegmentData::VideoSubset { source_video, .. } => Some(source_video),
            SegmentData::Image { source_image, .. } => Some(source_image),
            SegmentData::Music { audio_source } => Some(audio_source),
        }
    }

    /// Get the transform for this segment data (if applicable)
    pub fn transform(&self) -> Option<&Transform> {
        match self {
            SegmentData::VideoSubset { transform, .. } => transform.as_ref(),
            SegmentData::Image { transform, .. } => transform.as_ref(),
            SegmentData::Music { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_creation() {
        let mut timeline = Timeline::new();
        assert_eq!(timeline.segments.len(), 0);
        assert_eq!(timeline.total_duration(), 0.0);
    }

    #[test]
    fn test_add_segment() {
        let mut timeline = Timeline::new();
        let segment = Segment::new_video_subset(0.0, 10.0, 5.0, PathBuf::from("test.mp4"), None);
        timeline.add_segment(segment);
        assert_eq!(timeline.segments.len(), 1);
        assert_eq!(timeline.total_duration(), 10.0);
    }

    #[test]
    fn test_segments_in_range() {
        let mut timeline = Timeline::new();
        timeline.add_segment(Segment::new_video_subset(
            0.0,
            10.0,
            0.0,
            PathBuf::from("test.mp4"),
            None,
        ));
        timeline.add_segment(Segment::new_video_subset(
            10.0,
            5.0,
            10.0,
            PathBuf::from("test.mp4"),
            None,
        ));
        timeline.add_segment(Segment::new_video_subset(
            20.0,
            5.0,
            20.0,
            PathBuf::from("test.mp4"),
            None,
        ));

        let segments = timeline.segments_in_range(5.0, 12.0);
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn test_transform_identity() {
        let transform = Transform::new();
        assert!(transform.is_identity());

        let transform = Transform::with_scale(1.5);
        assert!(!transform.is_identity());
    }
}
