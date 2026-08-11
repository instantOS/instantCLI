//! Validated, track-oriented render timeline.
//!
//! Source coordinates and output coordinates deliberately use different
//! types. The base track owns paired video/dialogue sources; overlays, B-roll,
//! and music cannot enter the base concat by construction.

use std::marker::PhantomData;
use std::ops::{Add, Sub};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct MediaDuration(f64);

impl MediaDuration {
    pub fn seconds(self) -> f64 {
        self.0
    }
}

impl From<f64> for MediaDuration {
    fn from(value: f64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct SourceTime(f64);

impl SourceTime {
    pub fn seconds(self) -> f64 {
        self.0
    }
}

impl From<f64> for SourceTime {
    fn from(value: f64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct TimelineTime(f64);

impl TimelineTime {
    pub fn seconds(self) -> f64 {
        self.0
    }
}

impl From<f64> for TimelineTime {
    fn from(value: f64) -> Self {
        Self(value)
    }
}

impl Add<MediaDuration> for SourceTime {
    type Output = SourceTime;
    fn add(self, rhs: MediaDuration) -> Self::Output {
        SourceTime(self.0 + rhs.0)
    }
}

impl Add<MediaDuration> for TimelineTime {
    type Output = TimelineTime;
    fn add(self, rhs: MediaDuration) -> Self::Output {
        TimelineTime(self.0 + rhs.0)
    }
}

impl Sub<TimelineTime> for TimelineTime {
    type Output = MediaDuration;
    fn sub(self, rhs: TimelineTime) -> Self::Output {
        MediaDuration(self.0 - rhs.0)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SourceCoordinates;

#[derive(Clone, Copy, Debug, Default)]
pub struct TimelineCoordinates;

/// A range whose coordinate space is part of its type.
#[derive(Clone, Copy, Debug, Default)]
pub struct TimeRange<Coordinates> {
    pub start: f64,
    pub end: f64,
    coordinates: PhantomData<Coordinates>,
}

pub type SourceRange = TimeRange<SourceCoordinates>;
pub type TimelineRange = TimeRange<TimelineCoordinates>;

impl<Coordinates> TimeRange<Coordinates> {
    pub fn new(start: f64, end: f64) -> Self {
        Self {
            start,
            end,
            coordinates: PhantomData,
        }
    }

    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && self.end > other.start
    }

    pub fn overlap_seconds(self, other: Self) -> f64 {
        (self.end.min(other.end) - self.start.max(other.start)).max(0.0)
    }

    pub fn overlap_window(self, other: Self) -> Self {
        let start = self.start.max(other.start);
        Self::new(start, self.end.min(other.end).max(start))
    }
}

#[derive(Debug, Clone)]
pub struct AvSourceRef {
    pub video: PathBuf,
    pub audio: PathBuf,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct BaseAvClip {
    pub timeline_start: TimelineTime,
    pub duration: MediaDuration,
    pub source_start: SourceTime,
    pub source: AvSourceRef,
    pub mute_audio: bool,
}

#[cfg(test)]
impl BaseAvClip {
    pub fn new(
        timeline_start: f64,
        duration: f64,
        source_start: f64,
        source: AvSourceRef,
        mute_audio: bool,
    ) -> Self {
        Self {
            timeline_start: timeline_start.into(),
            duration: duration.into(),
            source_start: source_start.into(),
            source,
            mute_audio,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageOverlay {
    pub timeline_start: TimelineTime,
    pub duration: MediaDuration,
    pub source_image: PathBuf,
    pub transform: Option<Transform>,
}

#[derive(Debug, Clone)]
pub struct BrollClip {
    pub timeline_start: TimelineTime,
    pub duration: MediaDuration,
    pub source_start: SourceTime,
    pub source_video: PathBuf,
    pub transform: Option<Transform>,
}

#[derive(Debug, Clone)]
pub struct MusicClip {
    pub timeline_start: TimelineTime,
    pub duration: MediaDuration,
    pub source_start: SourceTime,
    pub audio_source: PathBuf,
}

#[cfg(test)]
impl MusicClip {
    pub fn new(timeline_start: f64, duration: f64, audio_source: PathBuf) -> Self {
        Self {
            timeline_start: timeline_start.into(),
            duration: duration.into(),
            source_start: 0.0.into(),
            audio_source,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    pub base: Vec<BaseAvClip>,
    pub overlays: Vec<ImageOverlay>,
    pub broll: Vec<BrollClip>,
    pub music: Vec<MusicClip>,
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_base(&mut self, clip: BaseAvClip) {
        self.base.push(clip);
    }

    pub fn add_overlay(&mut self, clip: ImageOverlay) {
        self.overlays.push(clip);
    }

    pub fn add_broll(&mut self, clip: BrollClip) {
        self.broll.push(clip);
    }

    pub fn add_music(&mut self, clip: MusicClip) {
        self.music.push(clip);
    }

    pub fn total_duration(&self) -> f64 {
        self.base
            .iter()
            .map(|clip| (clip.timeline_start + clip.duration).seconds())
            .fold(0.0, f64::max)
    }

    pub fn has_overlays(&self) -> bool {
        !self.overlays.is_empty()
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.base.is_empty(),
            "Render timeline has no base A/V track"
        );
        let mut expected = 0.0;
        for clip in &self.base {
            anyhow::ensure!(
                clip.timeline_start.seconds().is_finite()
                    && clip.source_start.seconds().is_finite()
                    && clip.duration.seconds().is_finite()
                    && clip.timeline_start.seconds() >= 0.0
                    && clip.source_start.seconds() >= 0.0
                    && clip.duration.seconds() > 0.0,
                "Base A/V clip has invalid time coordinates"
            );
            anyhow::ensure!(
                (clip.timeline_start.seconds() - expected).abs() <= 0.5 / 48_000.0,
                "Base A/V track is not contiguous: expected {:.6}s, found {:.6}s",
                expected,
                clip.timeline_start.seconds()
            );
            expected += clip.duration.seconds();
        }
        for (track, start, duration, source_start) in self
            .overlays
            .iter()
            .map(|clip| ("overlay", clip.timeline_start, clip.duration, None))
            .chain(self.broll.iter().map(|clip| {
                (
                    "B-roll",
                    clip.timeline_start,
                    clip.duration,
                    Some(clip.source_start),
                )
            }))
            .chain(self.music.iter().map(|clip| {
                (
                    "music",
                    clip.timeline_start,
                    clip.duration,
                    Some(clip.source_start),
                )
            }))
        {
            anyhow::ensure!(
                start.seconds().is_finite()
                    && duration.seconds().is_finite()
                    && start.seconds() >= 0.0
                    && duration.seconds() > 0.0
                    && source_start
                        .is_none_or(|time| { time.seconds().is_finite() && time.seconds() >= 0.0 })
                    && (start + duration).seconds() <= expected + 0.5 / 48_000.0,
                "{track} clip has invalid or out-of-bounds time coordinates"
            );
        }
        Ok(())
    }

    pub fn truncate_before(&self, seek_seconds: f64) -> Timeline {
        let seek = TimelineTime::from(seek_seconds);
        let mut result = Timeline::new();
        for clip in &self.base {
            let end = clip.timeline_start + clip.duration;
            if end <= seek {
                continue;
            }
            let mut clip = clip.clone();
            if clip.timeline_start < seek {
                let removed = seek - clip.timeline_start;
                clip.source_start = clip.source_start + removed;
                clip.duration = MediaDuration::from(clip.duration.seconds() - removed.seconds());
                clip.timeline_start = TimelineTime::from(0.0);
            } else {
                clip.timeline_start =
                    TimelineTime::from(clip.timeline_start.seconds() - seek_seconds);
            }
            result.base.push(clip);
        }
        for clip in &self.overlays {
            let Some((start, duration, _removed)) = truncate_window(
                clip.timeline_start.seconds(),
                clip.duration.seconds(),
                seek_seconds,
            ) else {
                continue;
            };
            let mut clip = clip.clone();
            clip.timeline_start = start.into();
            clip.duration = duration.into();
            result.overlays.push(clip);
        }
        for clip in &self.broll {
            let Some((start, duration, removed)) = truncate_window(
                clip.timeline_start.seconds(),
                clip.duration.seconds(),
                seek_seconds,
            ) else {
                continue;
            };
            let mut clip = clip.clone();
            clip.timeline_start = start.into();
            clip.duration = duration.into();
            clip.source_start = SourceTime::from(clip.source_start.seconds() + removed);
            result.broll.push(clip);
        }
        for clip in &self.music {
            let Some((start, duration, removed)) = truncate_window(
                clip.timeline_start.seconds(),
                clip.duration.seconds(),
                seek_seconds,
            ) else {
                continue;
            };
            let mut clip = clip.clone();
            clip.timeline_start = start.into();
            clip.duration = duration.into();
            clip.source_start = SourceTime::from(clip.source_start.seconds() + removed);
            result.music.push(clip);
        }
        result
    }
}

fn truncate_window(start: f64, duration: f64, seek: f64) -> Option<(f64, f64, f64)> {
    let end = start + duration;
    if end <= seek {
        return None;
    }
    let removed = (seek - start).clamp(0.0, duration);
    Some(((start - seek).max(0.0), duration - removed, removed))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Center,
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Debug, Clone)]
pub struct Transform {
    pub scale: Option<f32>,
    pub position: Option<Position>,
    pub translate: Option<(f32, f32)>,
    _coordinate_space: PhantomData<TimelineTime>,
}

impl Transform {
    pub fn new() -> Self {
        Self {
            scale: None,
            position: None,
            translate: None,
            _coordinate_space: PhantomData,
        }
    }

    pub fn from_parts(
        scale: Option<f32>,
        position: Option<Position>,
        translate: Option<(f32, f32)>,
    ) -> Self {
        Self {
            scale,
            position,
            translate,
            _coordinate_space: PhantomData,
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_contiguous_base_track() {
        let source = AvSourceRef {
            video: "v.mp4".into(),
            audio: "a.wav".into(),
            id: "a".into(),
        };
        let mut timeline = Timeline::new();
        timeline.add_base(BaseAvClip {
            timeline_start: 0.0.into(),
            duration: 1.0.into(),
            source_start: 2.0.into(),
            source,
            mute_audio: false,
        });
        assert!(timeline.validate().is_ok());
    }
}
