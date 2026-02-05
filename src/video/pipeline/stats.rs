use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ui::prelude::Level;

use crate::video::cli::StatsArgs;
use crate::video::document::{VideoMetadata, VideoSource, parse_video_document};
use crate::video::pipeline::report::{ReportLine, emit_report, format_report_lines};
use crate::video::planning::{TimelinePlan, plan_timeline};
use crate::video::render::paths::resolve_video_sources;
use crate::video::support::utils::canonicalize_existing;

pub fn handle_stats(args: StatsArgs) -> Result<()> {
    let report = build_stats_report(args)?;
    emit_report(&report);
    Ok(())
}

pub fn stats_report_lines(args: StatsArgs) -> Result<Vec<String>> {
    let report = build_stats_report(args)?;
    Ok(format_report_lines(&report))
}

fn build_stats_report(args: StatsArgs) -> Result<Vec<ReportLine>> {
    let mut report: Vec<ReportLine> = Vec::new();
    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    check_video_sources(&document.metadata, markdown_dir, &mut report);

    let plan = plan_timeline(&document)?;
    emit_plan_stats(&plan, &mut report);

    Ok(report)
}

fn check_video_sources(
    metadata: &VideoMetadata,
    markdown_dir: &Path,
    report: &mut Vec<ReportLine>,
) {
    match resolve_video_sources(metadata, markdown_dir) {
        Ok(resolved) => {
            if resolved.is_empty() {
                report.push(ReportLine::new(
                    Level::Warn,
                    "video.stats.video_metadata",
                    "No video sources configured in front matter",
                ));
            }
            for source in resolved {
                emit_source_status(&source, report);
            }
        }
        Err(error) => {
            report.push(ReportLine::new(
                Level::Error,
                "video.stats.video_metadata",
                format!("Unable to resolve source videos: {error}"),
            ));
        }
    };
}

fn emit_source_status(source: &VideoSource, report: &mut Vec<ReportLine>) {
    let exists = source.source.exists();
    report.push(ReportLine::new(
        if exists { Level::Success } else { Level::Warn },
        "video.stats.video",
        format!(
            "Source {} video {} {}",
            source.id,
            if exists { "found at" } else { "missing at" },
            source.source.display()
        ),
    ));
}

fn emit_plan_stats(plan: &TimelinePlan, report: &mut Vec<ReportLine>) {
    if plan.items.is_empty() {
        report.push(ReportLine::new(
            Level::Warn,
            "video.stats.empty",
            "No renderable blocks detected in the markdown file",
        ));
    }

    if plan.ignored_count == 0 {
        report.push(ReportLine::new(
            Level::Success,
            "video.stats.supported",
            "Markdown contains only supported editing instructions",
        ));
    } else {
        report.push(ReportLine::new(
            Level::Warn,
            "video.stats.unsupported",
            format!(
                "Markdown contains {count} block(s) that are currently unsupported",
                count = plan.ignored_count
            ),
        ));
    }

    report.push(ReportLine::new(
        Level::Info,
        "video.stats.counts",
        format!(
            "Segments: {segments}, Standalone slides: {slides}, Heading slides: {headings}, Overlay slides: {overlays}",
            segments = plan.segment_count,
            slides = plan.standalone_count,
            headings = plan.heading_count,
            overlays = plan.overlay_count,
        ),
    ));
}
