mod document;
mod ffmpeg;
mod logging;
mod mode;
mod output;
pub mod paths;
mod pipeline;
mod plan;
mod sources;
mod subtitles;
pub mod timeline;
mod timeline_builder;
mod transcripts;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::ui::prelude::Level;

pub(crate) use self::document::load_video_document;
use self::ffmpeg::services::{FfmpegRunner, SystemFfmpegRunner};
use self::logging::log_event;
pub use self::mode::RenderMode;
use self::output::prepare_output_destination;
use self::pipeline::{RenderPipeline, RenderPipelineParams};
pub(crate) use self::plan::build_timeline_plan;
pub(crate) use self::sources::resolve_video_sources;
use self::sources::{find_default_source, validate_timeline_sources};
use self::subtitles::generate_subtitle_file;
use self::timeline_builder::{SlideProvider, TimelineStats, build_nle_timeline};
pub(crate) use self::transcripts::load_transcript_cues;
use super::cli::RenderArgs;
use super::config::VideoConfig;
use super::support::ffmpeg::probe_video_dimensions;

use super::slides::SlideGenerator;
use super::support::utils::canonicalize_existing;

impl SlideProvider for SlideGenerator {
    fn overlay_slide_image(&self, markdown: &str) -> Result<PathBuf> {
        Ok(self.markdown_slide(markdown)?.image_path)
    }

    fn standalone_slide_video(&self, markdown: &str, duration: f64) -> Result<PathBuf> {
        let asset = self.markdown_slide(markdown)?;
        self.ensure_video_for_duration(&asset, duration)
    }
}

pub async fn handle_render(args: RenderArgs) -> Result<Option<PathBuf>> {
    let runner = SystemFfmpegRunner;
    handle_render_with_services(args, &runner).await
}

struct RenderProject {
    sources: Vec<crate::video::document::VideoSource>,
    cues: Vec<crate::video::support::transcript::TranscriptCue>,
    plan: crate::video::planning::TimelinePlan,
    default_source: crate::video::document::VideoSource,
    video_config: VideoConfig,
    project_dir: PathBuf,
}

async fn load_render_project(args: &RenderArgs) -> Result<RenderProject> {
    log_event(
        Level::Info,
        "video.render.start",
        "Preparing render (reading markdown, transcript, and assets)",
    );

    let markdown_path = canonicalize_existing(&args.markdown)?;
    let project_dir = markdown_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let document = load_video_document(&markdown_path)?;
    let video_config = VideoConfig::load()?;
    let sources = resolve_video_sources(&document.metadata, &project_dir, &video_config).await?;
    if sources.is_empty() {
        bail!("No video sources configured. Add `sources` in front matter before rendering.");
    }
    let cues = load_transcript_cues(&sources, &project_dir)?;
    validate_timeline_sources(&document, &sources, &cues)?;
    let plan = build_timeline_plan(&document, &cues, &markdown_path)?;
    let default_source = find_default_source(&document.metadata, &sources)?.clone();

    Ok(RenderProject {
        sources,
        cues,
        plan,
        default_source,
        video_config,
        project_dir,
    })
}

fn build_render_timeline(
    project: &RenderProject,
    render_mode: RenderMode,
) -> Result<(timeline::Timeline, (u32, u32))> {
    log_event(
        Level::Info,
        "video.render.probe",
        "Probing source video dimensions",
    );
    let (video_width, video_height) = probe_video_dimensions(&project.default_source.source)?;
    let (target_width, target_height) = render_mode.target_dimensions(video_width, video_height);

    let generator = SlideGenerator::new(target_width, target_height)?;

    log_event(
        Level::Info,
        "video.render.timeline.build",
        "Building render timeline (may generate slides)",
    );
    let (nle_timeline, stats) = build_nle_timeline(
        project.plan.clone(),
        &generator,
        &project.sources,
        &project.project_dir,
    )?;

    report_timeline_stats(&stats);

    Ok((nle_timeline, (target_width, target_height)))
}

fn execute_render(
    nle_timeline: timeline::Timeline,
    cues: &[crate::video::support::transcript::TranscriptCue],
    output_path: PathBuf,
    render_mode: RenderMode,
    target_dims: (u32, u32),
    video_config: VideoConfig,
    audio_source: PathBuf,
    burn_subtitles: bool,
    dry_run: bool,
    runner: &dyn FfmpegRunner,
) -> Result<Option<PathBuf>> {
    let (target_width, target_height) = target_dims;

    let subtitle_path = if burn_subtitles {
        log_event(
            Level::Info,
            "video.render.subtitles",
            format!("Generating ASS subtitles for {:?} mode", render_mode),
        );
        Some(generate_subtitle_file(
            &nle_timeline,
            cues,
            &output_path,
            (target_width, target_height),
            render_mode,
        )?)
    } else {
        None
    };

    let pipeline = RenderPipeline::new(RenderPipelineParams {
        output: output_path.clone(),
        timeline: nle_timeline,
        render_mode,
        target_width,
        target_height,
        config: video_config,
        audio_source,
        subtitle_path,
        runner,
    });

    log_event(
        Level::Info,
        "video.render.ffmpeg",
        "Preparing ffmpeg pipeline",
    );

    if dry_run {
        pipeline.print_command()?;
        log_event(
            Level::Info,
            "video.render.dry_run",
            "Dry run completed - ffmpeg command printed above",
        );
        return Ok(None);
    }

    log_event(
        Level::Info,
        "video.render.execute",
        "Starting ffmpeg render",
    );
    pipeline.execute()?;

    log_event(
        Level::Success,
        "video.render.success",
        format!("Rendered edited timeline to {}", output_path.display()),
    );

    Ok(Some(output_path))
}

async fn handle_render_with_services(
    args: RenderArgs,
    runner: &dyn FfmpegRunner,
) -> Result<Option<PathBuf>> {
    let project = load_render_project(&args).await?;

    let render_mode = if args.reels {
        RenderMode::Reels
    } else {
        RenderMode::Standard
    };

    let output_path = if args.precache_slides {
        None
    } else {
        Some(paths::resolve_output_path(
            args.out_file.as_ref(),
            &project.default_source.source,
            &project.project_dir,
            render_mode,
        )?)
    };

    if let Some(output_path) = &output_path {
        prepare_output_destination(output_path, &args, &project.default_source.source)?;
    }

    let (nle_timeline, target_dims) = build_render_timeline(&project, render_mode)?;

    if args.precache_slides {
        log_event(
            Level::Success,
            "video.render.precache_only",
            "Prepared slides in cache; skipping final render",
        );
        return Ok(None);
    }

    let Some(output_path) = output_path else {
        bail!("Output path is required when not pre-caching");
    };

    execute_render(
        nle_timeline,
        &project.cues,
        output_path,
        render_mode,
        target_dims,
        project.video_config,
        project.default_source.source.clone(),
        args.subtitles,
        args.dry_run,
        runner,
    )
}

fn report_timeline_stats(stats: &TimelineStats) {
    if stats.standalone_count > 0 {
        log_event(
            Level::Info,
            "video.render.slides.standalone",
            format!(
                "Generated {count} standalone slide(s)",
                count = stats.standalone_count
            ),
        );
    }

    if stats.overlay_count > 0 {
        log_event(
            Level::Info,
            "video.render.slides.overlay",
            format!(
                "Applied {count} overlay slide(s)",
                count = stats.overlay_count
            ),
        );
    }

    if stats.ignored_count > 0 {
        log_event(
            Level::Warn,
            "video.render.unhandled_blocks",
            format!(
                "Ignored {count} markdown block(s) that are not yet supported",
                count = stats.ignored_count
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::SegmentKind;
    use crate::video::document::VideoSource;
    use crate::video::planning::ClipPlan;
    use crate::video::planning::{StandalonePlan, TimelinePlan, TimelinePlanItem};
    use crate::video::render::timeline::{SegmentData, TimeWindow};
    use std::path::{Path, PathBuf};

    struct StubSlides;

    impl SlideProvider for StubSlides {
        fn overlay_slide_image(&self, _markdown: &str) -> Result<PathBuf> {
            anyhow::bail!("unexpected overlay slide generation")
        }

        fn standalone_slide_video(&self, _markdown: &str, _duration: f64) -> Result<PathBuf> {
            Ok(PathBuf::from("card.mp4"))
        }
    }

    #[test]
    fn inserts_heading_slides_between_clip_segments() {
        let source_video = Path::new("source.mp4");
        let project_dir = Path::new(".");

        let plan = TimelinePlan {
            items: vec![
                TimelinePlanItem::Clip(ClipPlan {
                    time_window: TimeWindow::new(0.0, 12.0),
                    kind: SegmentKind::Dialogue,
                    text: "hello world".to_string(),
                    overlay: None,
                    broll: None,
                    source_id: "a".to_string(),
                }),
                TimelinePlanItem::Standalone(StandalonePlan {
                    markdown: "# title card".to_string(),
                    duration_seconds: 2.0,
                }),
                TimelinePlanItem::Clip(ClipPlan {
                    time_window: TimeWindow::new(12.0, 20.0),
                    kind: SegmentKind::Dialogue,
                    text: "this is a test".to_string(),
                    overlay: None,
                    broll: None,
                    source_id: "a".to_string(),
                }),
            ],
            standalone_count: 1,
            overlay_count: 0,
            ignored_count: 0,
            heading_count: 1,
            segment_count: 2,
        };

        let sources = vec![VideoSource {
            id: "a".to_string(),
            name: Some("source".to_string()),
            source: source_video.to_path_buf(),
            transcript: PathBuf::from("source.json"),
            audio: PathBuf::from("source_audio.wav"),
            hash: None,
        }];

        let (timeline, _stats) =
            build_nle_timeline(plan, &StubSlides, &sources, project_dir).unwrap();

        assert_eq!(timeline.segments.len(), 3);

        let SegmentData::VideoSubset {
            start_time: clip1_source_start,
            source_video: clip1_source,
            mute_audio: clip1_mute,
            ..
        } = &timeline.segments[0].data
        else {
            panic!("expected first segment to be a video subset")
        };
        assert!((timeline.segments[0].start_time - 0.0).abs() < 1e-6);
        assert!((timeline.segments[0].duration - 12.0).abs() < 1e-6);
        assert!((*clip1_source_start - 0.0).abs() < 1e-6);
        assert_eq!(clip1_source, &PathBuf::from("source.mp4"));
        assert!(!clip1_mute);

        let SegmentData::VideoSubset {
            start_time: card_source_start,
            source_video: card_source,
            mute_audio: card_mute,
            ..
        } = &timeline.segments[1].data
        else {
            panic!("expected second segment to be a video subset")
        };
        assert!((timeline.segments[1].start_time - 12.0).abs() < 1e-6);
        assert!((timeline.segments[1].duration - 2.0).abs() < 1e-6);
        assert!((*card_source_start - 0.0).abs() < 1e-6);
        assert_eq!(card_source, &PathBuf::from("card.mp4"));
        assert!(*card_mute);

        let SegmentData::VideoSubset {
            start_time: clip2_source_start,
            source_video: clip2_source,
            mute_audio: clip2_mute,
            ..
        } = &timeline.segments[2].data
        else {
            panic!("expected third segment to be a video subset")
        };
        assert!((timeline.segments[2].start_time - 14.0).abs() < 1e-6);
        assert!((timeline.segments[2].duration - 8.0).abs() < 1e-6);
        assert!((*clip2_source_start - 12.0).abs() < 1e-6);
        assert_eq!(clip2_source, &PathBuf::from("source.mp4"));
        assert!(!clip2_mute);
    }

    #[test]
    fn test_render_mode_standard() {
        let mode = RenderMode::Standard;
        assert_eq!(mode.target_dimensions(1920, 1080), (1920, 1080));
        assert_eq!(mode.output_suffix(), "_edit");
        assert!(!mode.requires_padding());
        assert_eq!(mode.vertical_offset_pct(), 0.5);
    }

    #[test]
    fn test_render_mode_reels() {
        let mode = RenderMode::Reels;
        assert_eq!(mode.target_dimensions(1920, 1080), (1080, 1920));
        assert_eq!(mode.output_suffix(), "_reels");
        assert!(mode.requires_padding());
        assert_eq!(mode.vertical_offset_pct(), 0.1);
    }

    #[test]
    fn test_render_mode_default() {
        let mode = RenderMode::default();
        assert_eq!(mode, RenderMode::Standard);
    }
}
