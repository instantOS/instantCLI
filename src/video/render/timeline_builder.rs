use std::path::Path;

use anyhow::{Result, anyhow};

use crate::video::document::VideoSource;
use crate::video::planning::{BrollPlan, StandalonePlan, TimelinePlan, TimelinePlanItem};
use crate::video::render::ffmpeg::services::{DefaultMusicSourceResolver, MusicSourceResolver};
use crate::video::render::timeline::{Segment, Timeline};

pub(super) trait SlideProvider {
    fn overlay_slide_image(&self, markdown: &str) -> Result<std::path::PathBuf>;
    fn standalone_slide_video(&self, markdown: &str, duration: f64) -> Result<std::path::PathBuf>;
}

pub(super) struct TimelineStats {
    pub(super) standalone_count: usize,
    pub(super) overlay_count: usize,
    pub(super) ignored_count: usize,
}

/// Build an NLE timeline from the timeline plan
pub(super) fn build_nle_timeline(
    plan: TimelinePlan,
    generator: &dyn SlideProvider,
    sources: &[VideoSource],
    markdown_dir: &Path,
) -> Result<(Timeline, TimelineStats)> {
    let stats = TimelineStats {
        standalone_count: plan.standalone_count,
        overlay_count: plan.overlay_count,
        ignored_count: plan.ignored_count,
    };

    let mut state = TimelineBuildState::new(markdown_dir);

    for item in plan.items {
        state.apply_plan_item(item, generator, sources)?;
    }

    state.finalize();

    // Set has_overlays flag based on plan
    let has_overlays = plan.overlay_count > 0;
    let mut timeline = state.timeline;
    timeline.has_overlays = has_overlays;

    Ok((timeline, stats))
}

struct TimelineBuildState {
    timeline: Timeline,
    current_time: f64,
    music_resolver: Box<dyn MusicSourceResolver>,
    active_music: Option<ActiveMusic>,
}

impl TimelineBuildState {
    fn new(markdown_dir: &Path) -> Self {
        Self {
            timeline: Timeline::new(),
            current_time: 0.0,
            music_resolver: Box::new(DefaultMusicSourceResolver::new(markdown_dir)),
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
        let source_video = source.source.clone();
        let audio_source = source.audio.clone();
        let duration = clip_plan.end - clip_plan.start;

        let segment = Segment::new_video_subset(
            self.current_time,
            duration,
            clip_plan.start,
            source_video,
            audio_source,
            clip_plan.source_id.clone(),
            None,
            false,
        );
        self.timeline.add_segment(segment);

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

        let total_clip_duration: f64 = broll_plan.clips.iter().map(|c| c.end - c.start).sum();

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

            let clip_natural_duration = clip.end - clip.start;
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

            let segment = Segment::new_broll(
                broll_start + elapsed,
                clip_duration,
                clip.start,
                source.source.clone(),
                clip.source_id.clone(),
                None,
            );
            self.timeline.add_segment(segment);
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
        let overlay_segment = Segment::new_image(self.current_time, duration, image_path, None);
        self.timeline.add_segment(overlay_segment);
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

        let segment = Segment::new_video_subset(
            self.current_time,
            duration,
            0.0,
            video_path.clone(),
            video_path,
            "__slide".to_string(),
            None,
            true,
        );
        self.timeline.add_segment(segment);
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
        timeline.add_segment(Segment::new_music(state.start_time, duration, state.path));
    }
}
