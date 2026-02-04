use std::path::Path;

use anyhow::{Result, bail};

use crate::ui::prelude::{Level, emit};

use crate::video::cli::CheckArgs;
use crate::video::planning::TimelinePlanItem;
use crate::video::render::{
    build_timeline_plan, load_transcript_cues, load_video_document, resolve_video_sources,
};
use crate::video::support::ffmpeg::probe_video_dimensions;
use crate::video::support::utils::canonicalize_existing;

pub fn handle_check(args: CheckArgs) -> Result<()> {
    macro_rules! log {
        ($level:expr, $code:expr, $($arg:tt)*) => {
            emit($level, $code, &format!($($arg)*), None);
        };
    }

    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));

    // Load video document using shared helper
    let document = load_video_document(&markdown_path)?;

    // Resolve video sources
    let sources = resolve_video_sources(&document.metadata, markdown_dir)?;
    if sources.is_empty() {
        bail!("No video sources configured in front matter.");
    }

    // Load transcript cues using shared helper
    let cues = load_transcript_cues(&sources, markdown_dir)?;

    // Build timeline plan using shared helper
    let plan = build_timeline_plan(&document, &cues, &markdown_path)?;

    let default_source = sources
        .iter()
        .find(|source| {
            document
                .metadata
                .default_source
                .as_ref()
                .map(|id| id == &source.id)
                .unwrap_or(true)
        })
        .unwrap_or(&sources[0]);
    let (video_width, video_height) = probe_video_dimensions(&default_source.source)?;

    let duration_seconds = plan_duration_seconds(&plan);
    let duration_pretty = format_duration(duration_seconds);
    let pause_count = plan.standalone_count.saturating_sub(plan.heading_count);
    let unsupported_blocks = plan.ignored_count;

    log!(
        Level::Success,
        "video.check.valid",
        "{} is valid video markdown",
        markdown_path.display()
    );

    log!(
        Level::Info,
        "video.check.inputs",
        "Default video: {} ({}x{})\nSources: {} ({} cue(s))",
        default_source.source.display(),
        video_width,
        video_height,
        sources.len(),
        cues.len()
    );

    log!(
        Level::Info,
        "video.check.duration",
        "Planned output duration: {duration_pretty} (~{seconds:.1}s)",
        seconds = duration_seconds
    );

    log!(
        Level::Info,
        "video.check.counts",
        "Clips: {segments}, Overlay slides: {overlays}, Heading slides: {headings}, Pause slides: {pauses}",
        segments = plan.segment_count,
        overlays = plan.overlay_count,
        headings = plan.heading_count,
        pauses = pause_count,
    );

    if unsupported_blocks == 0 {
        log!(
            Level::Success,
            "video.check.supported",
            "All markdown blocks are supported"
        );
    } else {
        log!(
            Level::Warn,
            "video.check.partial_support",
            "{unsupported_blocks} unsupported block(s) will be ignored during render",
        );
    }

    Ok(())
}

fn plan_duration_seconds(plan: &crate::video::planning::TimelinePlan) -> f64 {
    plan.items
        .iter()
        .map(|item| match item {
            TimelinePlanItem::Clip(clip) => (clip.end - clip.start).max(0.0),
            TimelinePlanItem::Standalone(standalone) => match standalone {
                crate::video::planning::StandalonePlan::Heading { .. } => 2.0,
                crate::video::planning::StandalonePlan::Pause {
                    duration_seconds, ..
                } => *duration_seconds,
            },
            TimelinePlanItem::Music(_) => 0.0,
        })
        .sum::<f64>()
}

fn format_duration(seconds: f64) -> String {
    let total_seconds = seconds.round().max(0.0) as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}
