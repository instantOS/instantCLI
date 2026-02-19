//! Render Timeline Module
//!
//! This module defines the concrete timeline structure used for actual video rendering.
//! It represents a non-linear editor style timeline where segments are arranged
//! in sequence with precise timing and content data.
//!
//! The render timeline is the final output of the video planning pipeline:
//! 1. Markdown document → Video plan (planner module)
//! 2. Video plan → Render timeline (this module)
//! 3. Render timeline → Final video output
//!
//! Each segment contains:
//! - Start time and duration in the final video
//! - Content data (video clips, images, audio)
//! - Optional transforms (scale, rotate, position)

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default)]
pub struct TimeWindow {
    pub start: f64,
    pub end: f64,
}

impl TimeWindow {
    pub fn new(start: f64, end: f64) -> Self {
        Self { start, end }
    }

    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && self.end > other.start
    }

    pub fn overlap_seconds(self, other: Self) -> f64 {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (end - start).max(0.0)
    }

    pub fn overlap_window(self, other: Self) -> Self {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        Self::new(start, end.max(start))
    }
}

impl Segment {
    pub fn time_window(&self) -> TimeWindow {
        TimeWindow::new(self.start_time, self.end_time())
    }
}

/// Non-linear editor style timeline structure
/// This represents a sequence of segments that will be rendered in order
#[derive(Debug, Clone)]
pub struct Timeline {
    pub segments: Vec<Segment>,
    /// Track if timeline contains overlay segments
    pub has_overlays: bool,
}

/// Groups the video file, audio file, and source identifier that always travel together.
#[derive(Debug, Clone)]
pub struct AvSourceRef {
    pub video: PathBuf,
    pub audio: PathBuf,
    pub id: String,
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
        /// Video+audio source reference
        source: AvSourceRef,
        /// Optional transform to apply to this video segment
        transform: Option<Transform>,
        /// If true, no dialogue audio should be played for this segment (e.g., title cards)
        mute_audio: bool,
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
    /// B-roll video overlay (muted video that plays on top of the main video)
    Broll {
        /// Start time in the source video (in seconds)
        start_time: f64,
        /// Path to the source video file
        source_video: PathBuf,
        /// Source identifier for finding the video file
        source_id: String,
        /// Optional transform to apply to this b-roll segment
        transform: Option<Transform>,
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
            has_overlays: false,
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
    pub fn segments_in_range(&self, window: TimeWindow) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| {
                let seg_window = s.time_window();
                seg_window.overlaps(window)
            })
            .collect()
    }

    pub fn video_segments(&self) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| matches!(s.data, SegmentData::VideoSubset { .. }))
            .collect()
    }

    pub fn overlay_segments(&self) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| matches!(s.data, SegmentData::Image { .. }))
            .collect()
    }

    pub fn music_segments(&self) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| matches!(s.data, SegmentData::Music { .. }))
            .collect()
    }

    pub fn broll_segments(&self) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| matches!(s.data, SegmentData::Broll { .. }))
            .collect()
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    /// Create a new timeline starting from `seek_time`.
    ///
    /// - Drops segments that end at or before the seek point
    /// - Trims the first overlapping segment (shortens duration, advances source start)
    /// - Shifts all remaining segment start times so the timeline begins at 0
    pub fn truncate_before(&self, seek_time: f64) -> Timeline {
        let mut segments = Vec::new();

        for seg in &self.segments {
            let seg_end = seg.start_time + seg.duration;

            // Drop segments that end at or before the seek point
            if seg_end <= seek_time {
                continue;
            }

            if seg.start_time >= seek_time {
                // Segment starts at or after seek — keep as-is but shift start_time
                segments.push(Segment {
                    start_time: seg.start_time - seek_time,
                    duration: seg.duration,
                    data: seg.data.clone(),
                });
            } else {
                // Segment overlaps the seek point — trim the beginning
                let trim_amount = seek_time - seg.start_time;
                let new_duration = seg.duration - trim_amount;
                let new_data = seg.data.advance_start(trim_amount);

                segments.push(Segment {
                    start_time: 0.0,
                    duration: new_duration,
                    data: new_data,
                });
            }
        }

        Timeline {
            segments,
            has_overlays: self.has_overlays,
        }
    }
}

impl Segment {
    /// Create a new video subset segment
    pub fn new_video_subset(
        start_time: f64,
        duration: f64,
        source_start: f64,
        source: AvSourceRef,
        transform: Option<Transform>,
        mute_audio: bool,
    ) -> Self {
        Segment {
            start_time,
            duration,
            data: SegmentData::VideoSubset {
                start_time: source_start,
                source,
                transform,
                mute_audio,
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

    /// Create a new B-roll segment (muted video overlay)
    pub fn new_broll(
        start_time: f64,
        duration: f64,
        source_start: f64,
        source_video: PathBuf,
        source_id: String,
        transform: Option<Transform>,
    ) -> Self {
        Segment {
            start_time,
            duration,
            data: SegmentData::Broll {
                start_time: source_start,
                source_video,
                source_id,
                transform,
            },
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
            SegmentData::VideoSubset { source, .. } => Some(&source.video),
            SegmentData::Image { source_image, .. } => Some(source_image),
            SegmentData::Music { audio_source } => Some(audio_source),
            SegmentData::Broll { source_video, .. } => Some(source_video),
        }
    }

    pub fn audio_source(&self) -> Option<&PathBuf> {
        match self {
            SegmentData::VideoSubset { source, .. } => Some(&source.audio),
            _ => None,
        }
    }

    /// Get the transform for this segment data (if applicable)
    pub fn transform(&self) -> Option<&Transform> {
        match self {
            SegmentData::VideoSubset { transform, .. } => transform.as_ref(),
            SegmentData::Image { transform, .. } => transform.as_ref(),
            SegmentData::Music { .. } => None,
            SegmentData::Broll { transform, .. } => transform.as_ref(),
        }
    }

    /// Get the source start time (position in source file) for this segment.
    pub fn source_start_time(&self) -> Option<f64> {
        match self {
            SegmentData::VideoSubset { start_time, .. } => Some(*start_time),
            SegmentData::Broll { start_time, .. } => Some(*start_time),
            _ => None,
        }
    }

    /// Return a copy with the source start time advanced by `amount` seconds.
    /// For VideoSubset and Broll, this moves the source playback start forward.
    /// For Image and Music, data is returned unchanged (no seekable source offset).
    fn advance_start(&self, amount: f64) -> SegmentData {
        match self {
            SegmentData::VideoSubset {
                start_time,
                source,
                transform,
                mute_audio,
            } => SegmentData::VideoSubset {
                start_time: start_time + amount,
                source: source.clone(),
                transform: transform.clone(),
                mute_audio: *mute_audio,
            },
            SegmentData::Broll {
                start_time,
                source_video,
                source_id,
                transform,
            } => SegmentData::Broll {
                start_time: start_time + amount,
                source_video: source_video.clone(),
                source_id: source_id.clone(),
                transform: transform.clone(),
            },
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_creation() {
        let timeline = Timeline::new();
        assert_eq!(timeline.segments.len(), 0);
        assert_eq!(timeline.total_duration(), 0.0);
    }

    #[test]
    fn test_add_segment() {
        let mut timeline = Timeline::new();
        let segment = Segment::new_video_subset(
            0.0,
            10.0,
            5.0,
            AvSourceRef {
                video: PathBuf::from("test.mp4"),
                audio: PathBuf::from("test.mp4"),
                id: "a".to_string(),
            },
            None,
            false,
        );
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
            AvSourceRef {
                video: PathBuf::from("test.mp4"),
                audio: PathBuf::from("test.mp4"),
                id: "a".to_string(),
            },
            None,
            false,
        ));
        timeline.add_segment(Segment::new_video_subset(
            10.0,
            5.0,
            10.0,
            AvSourceRef {
                video: PathBuf::from("test.mp4"),
                audio: PathBuf::from("test.mp4"),
                id: "a".to_string(),
            },
            None,
            false,
        ));
        timeline.add_segment(Segment::new_video_subset(
            20.0,
            5.0,
            20.0,
            AvSourceRef {
                video: PathBuf::from("test.mp4"),
                audio: PathBuf::from("test.mp4"),
                id: "a".to_string(),
            },
            None,
            false,
        ));

        let segments = timeline.segments_in_range(TimeWindow::new(5.0, 12.0));
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn test_transform_identity() {
        let transform = Transform::new();
        assert!(transform.is_identity());

        let transform = Transform::with_scale(1.5);
        assert!(!transform.is_identity());
    }

    #[test]
    fn test_truncate_before_drops_early_segments() {
        let mut timeline = Timeline::new();
        // Segment 0-10s (source starts at 0)
        timeline.add_segment(Segment::new_video_subset(
            0.0,
            10.0,
            0.0,
            AvSourceRef {
                video: PathBuf::from("test.mp4"),
                audio: PathBuf::from("test.mp4"),
                id: "a".to_string(),
            },
            None,
            false,
        ));
        // Segment 10-20s (source starts at 10)
        timeline.add_segment(Segment::new_video_subset(
            10.0,
            10.0,
            10.0,
            AvSourceRef {
                video: PathBuf::from("test.mp4"),
                audio: PathBuf::from("test.mp4"),
                id: "a".to_string(),
            },
            None,
            false,
        ));
        // Segment 20-25s (source starts at 20)
        timeline.add_segment(Segment::new_video_subset(
            20.0,
            5.0,
            20.0,
            AvSourceRef {
                video: PathBuf::from("test.mp4"),
                audio: PathBuf::from("test.mp4"),
                id: "a".to_string(),
            },
            None,
            false,
        ));

        // Seek to 15s: first seg dropped, second seg trimmed, third kept
        let truncated = timeline.truncate_before(15.0);
        assert_eq!(truncated.segments.len(), 2);

        // First remaining segment: was 10-20, trimmed to 15-20
        // start_time shifted to 0, duration = 5s, source start advanced to 15
        let seg0 = &truncated.segments[0];
        assert_eq!(seg0.start_time, 0.0);
        assert_eq!(seg0.duration, 5.0);
        if let SegmentData::VideoSubset { start_time, .. } = &seg0.data {
            assert_eq!(*start_time, 15.0);
        } else {
            panic!("Expected VideoSubset");
        }

        // Second remaining segment: was at 20s, shifted to 5s
        let seg1 = &truncated.segments[1];
        assert_eq!(seg1.start_time, 5.0);
        assert_eq!(seg1.duration, 5.0);
    }
}
