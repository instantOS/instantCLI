use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::ui::prelude::{Level, emit};

use super::cli::CheckArgs;
use super::document::parse_video_document;
use super::ffmpeg::probe_video_dimensions;
use super::render::{resolve_transcript_path, resolve_video_path};
use super::srt::parse_srt;
use super::utils::canonicalize_existing;
use super::video_planner::{TimelinePlanItem, align_plan_with_subtitles, plan_timeline};

//TODO: break this function into smaller pieces
pub fn handle_check(args: CheckArgs) -> Result<()> {
    macro_rules! log {
        ($level:expr, $code:expr, $($arg:tt)*) => {
            emit($level, $code, &format!($($arg)*), None);
        };
    }

    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));

    let video_path = resolve_video_path(&document.metadata, markdown_dir)?;
    let video_path = canonicalize_existing(&video_path)?;
    let (video_width, video_height) = probe_video_dimensions(&video_path)?;

    let transcript_path = resolve_transcript_path(&document.metadata, markdown_dir)?;
    let transcript_path = canonicalize_existing(&transcript_path)?;
    let transcript_contents = fs::read_to_string(&transcript_path).with_context(|| {
        format!(
            "Failed to read transcript file {}",
            transcript_path.display()
        )
    })?;

    let cues = parse_srt(&transcript_contents)?;

    let mut plan = plan_timeline(&document)?;

    if plan.items.is_empty() {
        return Err(anyhow!(
            "No renderable blocks found in {}. Ensure the markdown contains timestamp code spans or headings.",
            markdown_path.display()
        ));
    }

    let unsupported_blocks = plan.ignored_count;

    align_plan_with_subtitles(&mut plan, &cues)?;

    let duration_seconds = plan_duration_seconds(&plan);
    let duration_pretty = format_duration(duration_seconds);
    let pause_count = plan.standalone_count.saturating_sub(plan.heading_count);

    log!(
        Level::Success,
        "video.check.valid",
        "{} is valid video markdown",
        markdown_path.display()
    );

    log!(
        Level::Info,
        "video.check.inputs",
        "Video: {} ({}x{})\nTranscript: {} ({} cue(s))",
        video_path.display(),
        video_width,
        video_height,
        transcript_path.display(),
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
        "Clips: {segments}, Overlays: {overlays}, Heading cards: {headings}, Pause cards: {pauses}",
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

fn plan_duration_seconds(plan: &super::video_planner::TimelinePlan) -> f64 {
    plan.items
        .iter()
        .map(|item| match item {
            TimelinePlanItem::Clip(clip) => (clip.end - clip.start).max(0.0),
            TimelinePlanItem::Standalone(standalone) => match standalone {
                super::video_planner::StandalonePlan::Heading { .. } => 2.0,
                super::video_planner::StandalonePlan::Pause { .. } => 5.0,
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
