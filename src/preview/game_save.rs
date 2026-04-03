use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::arch::dualboot::types::format_size;
use crate::game::utils::save_files::format_system_time_for_display;
use crate::preview::{GameSavePreviewPayload, PreviewContext};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewWriter;

const SIZE_SCAN_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;
const CONTENT_PREVIEW_LIMIT: usize = 30;

pub(crate) fn render_game_save_preview(ctx: &PreviewContext) -> Result<String> {
    let mut writer = PreviewWriter::collect();
    render_game_save_preview_impl(ctx, &mut writer)?;
    Ok(writer.build_string())
}

pub(crate) fn render_game_save_preview_streaming(ctx: &PreviewContext) -> Result<()> {
    let mut writer = PreviewWriter::streaming();
    render_game_save_preview_impl(ctx, &mut writer)
}

fn render_game_save_preview_impl(ctx: &PreviewContext, writer: &mut PreviewWriter) -> Result<()> {
    let Some(key) = ctx.key() else {
        writer.header(NerdFont::Warning, "No game selected");
        return Ok(());
    };

    let payload: GameSavePreviewPayload =
        serde_json::from_str(key).context("Failed to parse game save preview payload")?;

    writer
        .header(platform_icon(&payload.platform_short), &payload.name)
        .field("Platform", &payload.platform)
        .field("Save path", &payload.save_path);

    if let Some(game_path) = &payload.game_path {
        writer.field("Game path", game_path);
    }

    if let Some(prefix_path) = &payload.prefix_path {
        writer.field("Prefix path", prefix_path);
    }

    if payload.existing {
        writer.line(
            colors::GREEN,
            Some(NerdFont::Check),
            &format!(
                "Already tracked as {}",
                payload.tracked_name.as_deref().unwrap_or(&payload.name)
            ),
        );
    }

    writer.blank().separator().blank();

    let path = Path::new(&payload.save_path);
    if !path.exists() {
        writer
            .line(
                colors::RED,
                Some(NerdFont::Warning),
                "Detected path does not exist",
            )
            .blank()
            .subtext(&payload.save_path);
        return Ok(());
    }

    let metadata =
        fs::metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .map(|time| format_system_time_for_display(Some(time)))
        .unwrap_or_else(|| "Unknown".to_string());

    if metadata.is_file() {
        writer
            .field("Type", "File")
            .field("Size", &format_size(metadata.len()))
            .field("Modified", &modified)
            .blank()
            .text("Sibling entries:");
        for entry in list_parent_entries(path)? {
            writer.bullet(&entry);
        }
        return Ok(());
    }

    let summary = summarize_directory(path)?;
    let size_label = if summary.reached_limit {
        format!("{}+ (stopped at limit)", format_size(summary.total_size))
    } else {
        format_size(summary.total_size)
    };

    writer
        .field("Type", "Directory")
        .field("Files", &summary.file_count.to_string())
        .field("Size", &size_label)
        .field("Modified", &modified)
        .blank()
        .text("Contents:");

    for entry in list_directory_entries(path)? {
        writer.bullet(&entry);
    }

    if summary.reached_limit {
        writer.blank().subtext(&format!(
            "Recursive size scan stopped after {} to keep preview responsive.",
            format_size(SIZE_SCAN_LIMIT_BYTES)
        ));
    }

    Ok(())
}

fn platform_icon(platform_short: &str) -> NerdFont {
    match platform_short {
        "Epic" => NerdFont::Windows,
        "Steam" => NerdFont::Steam,
        "Wine" | "Faugus" => NerdFont::Wine,
        _ => NerdFont::Gamepad,
    }
}

struct DirectorySummary {
    total_size: u64,
    file_count: u64,
    reached_limit: bool,
}

fn summarize_directory(path: &Path) -> Result<DirectorySummary> {
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;

    for entry in WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            total_size = total_size.saturating_add(metadata.len());
            file_count += 1;
            if total_size >= SIZE_SCAN_LIMIT_BYTES {
                return Ok(DirectorySummary {
                    total_size,
                    file_count,
                    reached_limit: true,
                });
            }
        }
    }

    Ok(DirectorySummary {
        total_size,
        file_count,
        reached_limit: false,
    })
}

fn list_directory_entries(path: &Path) -> Result<Vec<String>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(path)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();

    let mut lines = Vec::new();
    for entry in entries.into_iter().take(CONTENT_PREVIEW_LIMIT) {
        let name = entry
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?")
            .to_string();
        let metadata = fs::metadata(&entry).ok();
        let detail = match metadata {
            Some(metadata) if metadata.is_file() => {
                format!("file, {}", format_size(metadata.len()))
            }
            Some(metadata) if metadata.is_dir() => "dir".to_string(),
            _ => "other".to_string(),
        };
        lines.push(format!("{name} ({detail})"));
    }

    if lines.is_empty() {
        lines.push("(empty directory)".to_string());
    } else if fs::read_dir(path)?.count() > CONTENT_PREVIEW_LIMIT {
        lines.push(format!(
            "... and more (showing first {CONTENT_PREVIEW_LIMIT})"
        ));
    }

    Ok(lines)
}

fn list_parent_entries(path: &Path) -> Result<Vec<String>> {
    let Some(parent) = path.parent() else {
        return Ok(vec!["(no parent directory)".to_string()]);
    };

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let mut entries: Vec<PathBuf> = fs::read_dir(parent)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();

    let mut lines = Vec::new();
    for entry in entries.into_iter().take(CONTENT_PREVIEW_LIMIT) {
        let name = entry
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?")
            .to_string();
        let selected = if name == file_name { "selected, " } else { "" };
        let metadata = fs::metadata(&entry).ok();
        let detail = match metadata {
            Some(metadata) if metadata.is_file() => {
                format!("{selected}file, {}", format_size(metadata.len()))
            }
            Some(metadata) if metadata.is_dir() => format!("{selected}dir"),
            _ => format!("{selected}other"),
        };
        lines.push(format!("{name} ({detail})"));
    }

    if lines.is_empty() {
        lines.push("(parent directory is empty)".to_string());
    } else if fs::read_dir(parent)?.count() > CONTENT_PREVIEW_LIMIT {
        lines.push(format!(
            "... and more (showing first {CONTENT_PREVIEW_LIMIT})"
        ));
    }

    Ok(lines)
}
