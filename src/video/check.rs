use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::ui::prelude::{Level, emit};

use super::cli::CheckArgs;
use super::document::parse_video_document;
use super::render::resolve_transcript_path;
use super::srt::parse_srt;
use super::timeline::{TimelinePlanItem, align_plan_with_subtitles, plan_timeline};
use super::utils::canonicalize_existing;

pub fn handle_check(args: CheckArgs) -> Result<()> {
    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));

    let transcript_path = resolve_transcript_path(&document.metadata, markdown_dir)?;
    let transcript_path = canonicalize_existing(&transcript_path)?;
    let transcript_contents = fs::read_to_string(&transcript_path)
        .with_context(|| format!("Failed to read transcript file {}", transcript_path.display()))?;

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

    emit(
        Level::Success,
        "video.check.valid",
        &format!("{} is valid video markdown", markdown_path.display()),
        None,
    );

    emit(
        Level::Info,
        "video.check.duration",
        &format!("Planned output duration: {}", format_duration(duration_seconds)),
        None,
    );

    emit(
        Level::Info,
        "video.check.counts",
        &format!(
            "Segments: {segments}, Headings: {headings}, Title cards: {titlecards}, Overlays: {overlays}",
            segments = plan.segment_count,
            headings = plan.heading_count,
            titlecards = plan.standalone_count,
            overlays = plan.overlay_count,
        ),
        None,
    );

    if unsupported_blocks == 0 {
        emit(
            Level::Success,
            "video.check.supported",
            "All markdown blocks are supported",
            None,
        );
    } else {
        emit(
            Level::Warn,
            "video.check.partial_support",
            &format!(
                "{unsupported_blocks} unsupported block(s) will be ignored during render",
            ),
            None,
        );
    }

    Ok(())
}

fn plan_duration_seconds(plan: &super::timeline::TimelinePlan) -> f64 {
    plan.items
        .iter()
        .filter_map(|item| match item {
            TimelinePlanItem::Clip(clip) => Some((clip.end - clip.start).max(0.0)),
            TimelinePlanItem::Standalone(_) => None,
            TimelinePlanItem::Music(_) => None,
        })
        .sum::<f64>()
        + (plan.standalone_count as f64) * 5.0
        + (plan.heading_count as f64) * 2.0
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
