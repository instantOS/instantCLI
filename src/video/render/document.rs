use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ui::prelude::Level;
use crate::video::document::{VideoDocument, parse_video_document};
use crate::video::render::logging::log_event;

pub(crate) fn load_video_document(markdown_path: &Path) -> Result<VideoDocument> {
    log_event(
        Level::Info,
        "video.render.markdown.read",
        format!("Reading markdown from {}", markdown_path.display()),
    );

    let markdown_contents = fs::read_to_string(markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;

    log_event(
        Level::Info,
        "video.render.markdown.parse",
        "Parsing markdown into video edit instructions",
    );
    parse_video_document(&markdown_contents, markdown_path)
}
