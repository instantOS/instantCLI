use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ui::prelude::{emit, Level};

use crate::video::cli::StatsArgs;
use crate::video::document::parse_video_document;
use crate::video::planning::plan_timeline;
use crate::video::render::resolve_video_sources;
use crate::video::support::utils::canonicalize_existing;

pub fn handle_stats(args: StatsArgs) -> Result<()> {
    let markdown_path = canonicalize_existing(&args.markdown)?;
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    match resolve_video_sources(&document.metadata, markdown_dir) {
        Ok(sources) => {
            if sources.is_empty() {
                emit(
                    Level::Warn,
                    "video.stats.video_metadata",
                    "No video sources configured in front matter",
                    None,
                );
            }
            for source in sources {
                let exists = source.source.exists();
                emit(
                    if exists { Level::Success } else { Level::Warn },
                    "video.stats.video",
                    &format!(
                        "Source {} video {} {}",
                        source.id,
                        if exists { "found at" } else { "missing at" },
                        source.source.display()
                    ),
                    None,
                );
            }
        }
        Err(error) => {
            emit(
                Level::Error,
                "video.stats.video_metadata",
                &format!("Unable to resolve source videos: {error}"),
                None,
            );
        }
    };

    let plan = plan_timeline(&document)?;

    if plan.items.is_empty() {
        emit(
            Level::Warn,
            "video.stats.empty",
            "No renderable blocks detected in the markdown file",
            None,
        );
    }

    if plan.ignored_count == 0 {
        emit(
            Level::Success,
            "video.stats.supported",
            "Markdown contains only supported editing instructions",
            None,
        );
    } else {
        emit(
            Level::Warn,
            "video.stats.unsupported",
            &format!(
                "Markdown contains {count} block(s) that are currently unsupported",
                count = plan.ignored_count
            ),
            None,
        );
    }

    emit(
        Level::Info,
        "video.stats.counts",
        &format!(
            "Segments: {segments}, Standalone slides: {slides}, Heading slides: {headings}, Overlay slides: {overlays}",
            segments = plan.segment_count,
            slides = plan.standalone_count,
            headings = plan.heading_count,
            overlays = plan.overlay_count,
        ),
        None,
    );

    Ok(())
}
