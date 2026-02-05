use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::ui::prelude::Level;
use crate::video::render::logging::log_event;
use crate::video::render::mode::RenderMode;
use crate::video::render::timeline::Timeline;
use crate::video::subtitles::{AssStyle, generate_ass_file, remap_subtitles_to_timeline};

/// Generate an ASS subtitle file for the timeline.
pub(super) fn generate_subtitle_file(
    timeline: &Timeline,
    cues: &[crate::video::support::transcript::TranscriptCue],
    output_path: &Path,
    play_res: (u32, u32),
    render_mode: RenderMode,
) -> Result<PathBuf> {
    let remapped = remap_subtitles_to_timeline(timeline, cues);

    if remapped.is_empty() {
        log_event(
            Level::Warn,
            "video.render.subtitles.empty",
            "No subtitle cues found to burn into video",
        );
    } else {
        log_event(
            Level::Info,
            "video.render.subtitles.count",
            format!("Remapped {} subtitle entries to timeline", remapped.len()),
        );
    }

    // Select style based on render mode
    let style = match render_mode {
        RenderMode::Reels => AssStyle::for_reels(timeline.has_overlays),
        RenderMode::Standard => AssStyle::for_standard(),
    };
    let ass_content = generate_ass_file(&remapped, &style, play_res);

    // Write ASS file next to output with .ass extension
    let ass_path = output_path.with_extension("ass");
    fs::write(&ass_path, &ass_content)
        .with_context(|| format!("Failed to write subtitle file to {}", ass_path.display()))?;

    log_event(
        Level::Info,
        "video.render.subtitles.written",
        format!("Wrote subtitle file to {}", ass_path.display()),
    );

    Ok(ass_path)
}
