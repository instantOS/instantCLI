use std::path::Path;

use anyhow::{Result, bail};

use crate::ui::prelude::Level;

use crate::video::cli::CheckArgs;
use crate::video::document::VideoSource;
use crate::video::pipeline::report::{ReportLine, emit_report, format_report_lines};
use crate::video::planning::TimelinePlanItem;
use crate::video::render::{
    build_timeline_plan, load_transcript_cues, load_video_document, resolve_video_sources,
};
use crate::video::support::ffmpeg::probe_video_dimensions;
use crate::video::support::utils::canonicalize_existing;

pub async fn handle_check(args: CheckArgs) -> Result<()> {
    let report = build_check_report(args).await?;
    emit_report(&report);
    Ok(())
}

pub async fn check_report_lines(args: CheckArgs) -> Result<Vec<String>> {
    let report = build_check_report(args).await?;
    Ok(format_report_lines(&report))
}

async fn build_check_report(args: CheckArgs) -> Result<Vec<ReportLine>> {
    let mut report: Vec<ReportLine> = Vec::new();
    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));

    // Load video document using shared helper
    let document = load_video_document(&markdown_path)?;

    // Resolve video sources
    let config = crate::video::config::VideoConfig::load()?;
    let sources = resolve_video_sources(&document.metadata, markdown_dir, &config).await?;
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

    report.push(ReportLine::new(
        Level::Success,
        "video.check.valid",
        format!("{} is valid video markdown", markdown_path.display()),
    ));

    report.push(ReportLine::new(
        Level::Info,
        "video.check.inputs",
        format!(
            "Default video: {} ({}x{})\nSources: {} ({} cue(s))",
            default_source.source.display(),
            video_width,
            video_height,
            sources.len(),
            cues.len()
        ),
    ));

    for source in &sources {
        emit_source_status(source, &mut report);
    }

    report.push(ReportLine::new(
        Level::Info,
        "video.check.duration",
        format!(
            "Planned output duration: {duration_pretty} (~{seconds:.1}s)",
            seconds = duration_seconds
        ),
    ));

    report.push(ReportLine::new(
        Level::Info,
        "video.check.counts",
        format!(
            "Clips: {segments}, Overlay slides: {overlays}, Heading slides: {headings}, Pause slides: {pauses}",
            segments = plan.segment_count,
            overlays = plan.overlay_count,
            headings = plan.heading_count,
            pauses = pause_count,
        ),
    ));

    if unsupported_blocks == 0 {
        report.push(ReportLine::new(
            Level::Success,
            "video.check.supported",
            "All markdown blocks are supported",
        ));
    } else {
        report.push(ReportLine::new(
            Level::Warn,
            "video.check.partial_support",
            format!("{unsupported_blocks} unsupported block(s) will be ignored during render"),
        ));
    }

    Ok(report)
}

fn plan_duration_seconds(plan: &crate::video::planning::TimelinePlan) -> f64 {
    plan.items
        .iter()
        .map(|item| match item {
            TimelinePlanItem::Clip(clip) => clip.time_window.duration().max(0.0),
            TimelinePlanItem::Standalone(standalone) => standalone.duration_seconds,
            TimelinePlanItem::Music(_) => 0.0,
        })
        .sum::<f64>()
}

fn emit_source_status(source: &VideoSource, report: &mut Vec<ReportLine>) {
    let exists = source.source.exists();
    report.push(ReportLine::new(
        if exists { Level::Success } else { Level::Warn },
        "video.check.source",
        format!(
            "Source {} video {} {}",
            source.id,
            if exists { "found at" } else { "missing at" },
            source.source.display()
        ),
    ));
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
