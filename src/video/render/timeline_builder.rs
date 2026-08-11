use std::path::Path;

use anyhow::{Result, anyhow};

use crate::video::document::VideoSource;
use crate::video::planning::{BrollPlan, StandalonePlan, TimelinePlan, TimelinePlanItem};
use crate::video::render::ffmpeg::services::{DefaultMusicSourceResolver, MusicSourceResolver};
use crate::video::render::timeline::{
    AvSourceRef, BaseAvClip, BrollClip, ImageOverlay, MediaDuration, MusicClip, Position,
    SourceTime, Timeline, TimelineTime, Transform,
};

pub(super) trait SlideProvider {
    fn overlay_slide_image(&self, markdown: &str) -> Result<std::path::PathBuf>;
    fn standalone_slide_video(&self, markdown: &str, duration: f64) -> Result<std::path::PathBuf>;
}

/// Build an NLE timeline from the timeline plan
pub(super) fn build_nle_timeline(
    plan: TimelinePlan,
    generator: &dyn SlideProvider,
    sources: &[VideoSource],
    project_dir: &Path,
) -> Result<Timeline> {
    let mut state = TimelineBuildState::new(project_dir);

    for item in plan.items {
        state.apply_plan_item(item, generator, sources)?;
    }

    state.finalize();

    state.timeline.validate()?;
    Ok(state.timeline)
}

struct TimelineBuildState {
    timeline: Timeline,
    current_time: f64,
    music_resolver: Box<dyn MusicSourceResolver>,
    active_music: Option<ActiveMusic>,
}

impl TimelineBuildState {
    fn new(project_dir: &Path) -> Self {
        Self {
            timeline: Timeline::new(),
            current_time: 0.0,
            music_resolver: Box::new(DefaultMusicSourceResolver::new(project_dir)),
            active_music: None,
        }
    }

    fn apply_plan_item(
        &mut self,
        item: TimelinePlanItem,
        generator: &dyn SlideProvider,
        sources: &[VideoSource],
    ) -> Result<()> {
        match item {
            TimelinePlanItem::Clip(clip_plan) => self.add_clip(clip_plan, generator, sources),
            TimelinePlanItem::Standalone(standalone_plan) => {
                self.add_standalone(standalone_plan, generator)
            }
            TimelinePlanItem::Music(music_plan) => self.add_music_directive(music_plan),
        }
    }

    fn add_clip(
        &mut self,
        clip_plan: crate::video::planning::ClipPlan,
        generator: &dyn SlideProvider,
        sources: &[VideoSource],
    ) -> Result<()> {
        let source = sources
            .iter()
            .find(|source| source.id == clip_plan.source_id)
            .ok_or_else(|| {
                anyhow!(
                    "No source configured for segment source id `{}`",
                    clip_plan.source_id
                )
            })?;
        let duration = clip_plan.time_window.duration();

        self.timeline.add_base(BaseAvClip {
            timeline_start: TimelineTime::from(self.current_time),
            duration: MediaDuration::from(duration),
            source_start: SourceTime::from(clip_plan.time_window.start),
            source: AvSourceRef {
                video: source.source.clone(),
                audio: source.audio.clone(),
                id: clip_plan.source_id.clone(),
            },
            mute_audio: false,
        });

        if let Some(overlay_plan) = clip_plan.overlay {
            self.add_overlay(&overlay_plan.markdown, duration, generator)?;
        }

        if let Some(broll_plan) = clip_plan.broll {
            self.add_broll(&broll_plan, duration, sources)?;
        }

        self.current_time += duration;
        Ok(())
    }

    fn add_broll(
        &mut self,
        broll_plan: &BrollPlan,
        available_duration: f64,
        sources: &[VideoSource],
    ) -> Result<()> {
        if broll_plan.clips.is_empty() {
            return Ok(());
        }

        let total_clip_duration: f64 = broll_plan
            .clips
            .iter()
            .map(|c| c.time_window.duration())
            .sum();

        let broll_start = self.current_time;
        let mut elapsed = 0.0;

        for (i, clip) in broll_plan.clips.iter().enumerate() {
            let source = sources
                .iter()
                .find(|s| s.id == clip.source_id)
                .ok_or_else(|| {
                    anyhow!(
                        "No source configured for B-roll source id `{}`",
                        clip.source_id
                    )
                })?;

            let clip_natural_duration = clip.time_window.duration();
            let is_last = i == broll_plan.clips.len() - 1;

            let clip_duration = if is_last {
                if total_clip_duration <= available_duration {
                    available_duration - elapsed
                } else {
                    (available_duration - elapsed).max(0.0)
                }
            } else if elapsed + clip_natural_duration > available_duration {
                break;
            } else {
                clip_natural_duration
            };

            if clip_duration <= 0.0 {
                break;
            }

            self.timeline.add_broll(BrollClip {
                timeline_start: TimelineTime::from(broll_start + elapsed),
                duration: MediaDuration::from(clip_duration),
                source_start: SourceTime::from(clip.time_window.start),
                source_video: source.source.clone(),
                transform: to_render_transform(&clip.transform),
            });
            elapsed += clip_duration;

            if elapsed >= available_duration {
                break;
            }
        }

        Ok(())
    }

    fn add_overlay(
        &mut self,
        markdown: &str,
        duration: f64,
        generator: &dyn SlideProvider,
    ) -> Result<()> {
        let image_path = generator.overlay_slide_image(markdown)?;
        self.timeline.add_overlay(ImageOverlay {
            timeline_start: TimelineTime::from(self.current_time),
            duration: MediaDuration::from(duration),
            source_image: image_path,
            transform: None,
        });
        Ok(())
    }

    fn add_standalone(
        &mut self,
        standalone_plan: StandalonePlan,
        generator: &dyn SlideProvider,
    ) -> Result<()> {
        self.add_standalone_slide(
            &standalone_plan.markdown,
            standalone_plan.duration_seconds,
            generator,
        )
    }

    fn add_standalone_slide(
        &mut self,
        markdown: &str,
        duration: f64,
        generator: &dyn SlideProvider,
    ) -> Result<()> {
        let video_path = generator.standalone_slide_video(markdown, duration)?;

        self.timeline.add_base(BaseAvClip {
            timeline_start: TimelineTime::from(self.current_time),
            duration: MediaDuration::from(duration),
            source_start: SourceTime::from(0.0),
            source: AvSourceRef {
                video: video_path.clone(),
                audio: video_path,
                id: "__slide".to_string(),
            },
            mute_audio: true,
        });
        self.current_time += duration;
        Ok(())
    }

    fn add_music_directive(&mut self, music_plan: crate::video::planning::MusicPlan) -> Result<()> {
        finalize_music_segment(
            &mut self.timeline,
            &mut self.active_music,
            self.current_time,
        );
        let resolved = self.music_resolver.resolve(&music_plan.directive)?;
        self.active_music = resolved.map(|path| ActiveMusic {
            path,
            start_time: self.current_time,
        });
        Ok(())
    }

    fn finalize(&mut self) {
        finalize_music_segment(
            &mut self.timeline,
            &mut self.active_music,
            self.current_time,
        );
    }
}

/// Convert a document-layer TransformSpec into a render-layer Transform.
fn to_render_transform(
    spec: &crate::video::document::transform::TransformSpec,
) -> Option<Transform> {
    if spec.is_empty() {
        return None;
    }
    Some(Transform::from_parts(
        spec.scale,
        spec.position.map(|p| match p {
            crate::video::document::transform::Position::Center => Position::Center,
            crate::video::document::transform::Position::TopLeft => Position::TopLeft,
            crate::video::document::transform::Position::Top => Position::Top,
            crate::video::document::transform::Position::TopRight => Position::TopRight,
            crate::video::document::transform::Position::Right => Position::Right,
            crate::video::document::transform::Position::BottomRight => Position::BottomRight,
            crate::video::document::transform::Position::Bottom => Position::Bottom,
            crate::video::document::transform::Position::BottomLeft => Position::BottomLeft,
            crate::video::document::transform::Position::Left => Position::Left,
        }),
        None,
    ))
}

struct ActiveMusic {
    path: std::path::PathBuf,
    start_time: f64,
}

fn finalize_music_segment(
    timeline: &mut Timeline,
    active: &mut Option<ActiveMusic>,
    end_time: f64,
) {
    if let Some(state) = active.take()
        && end_time > state.start_time
    {
        let duration = end_time - state.start_time;
        timeline.add_music(MusicClip {
            timeline_start: TimelineTime::from(state.start_time),
            duration: MediaDuration::from(duration),
            source_start: SourceTime::from(0.0),
            audio_source: state.path,
        });
    }
}
