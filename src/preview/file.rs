//! File suggestion preview - generates rich previews based on file type/mimetype.

use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::common::format::format_size;
use crate::game::utils::save_files::format_system_time_for_display;
use crate::preview::PreviewContext;
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::video::support::ffmpeg::probe_media_metadata;

/// Render a file suggestion preview based on the file's mimetype.
/// Dispatches to specialized previews for video/audio, images, etc.
pub(crate) fn render_file_suggestion_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(path_str) = ctx.key() else {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::File, "No file selected")
            .build_string());
    };

    let path = Path::new(path_str);
    let columns = ctx.columns();

    if !path.exists() {
        return Ok(append_wrapped_path(
            PreviewBuilder::new().header(NerdFont::Warning, "File not found"),
            Path::new(path_str),
            columns,
        )
        .build_string());
    }

    let mimetype = detect_mimetype(path);
    let category = mimetype_category(&mimetype);

    match category {
        FileCategory::Video | FileCategory::Audio => render_media_preview(path, &mimetype, columns),
        FileCategory::Image => render_image_preview(path, &mimetype, columns),
        FileCategory::Text => render_text_preview(path, &mimetype, columns),
        FileCategory::Archive => render_archive_preview(path, &mimetype, columns),
        FileCategory::Pdf => render_pdf_preview(path, &mimetype, columns),
        FileCategory::Directory => render_directory_preview(path, columns),
        FileCategory::Other => render_generic_preview(path, &mimetype, columns),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileCategory {
    Video,
    Audio,
    Image,
    Text,
    Archive,
    Pdf,
    Directory,
    Other,
}

fn mimetype_category(mimetype: &str) -> FileCategory {
    if mimetype.starts_with("video/") {
        return FileCategory::Video;
    }
    if mimetype.starts_with("audio/") {
        return FileCategory::Audio;
    }
    if mimetype.starts_with("image/") {
        return FileCategory::Image;
    }
    if mimetype.starts_with("text/") {
        return FileCategory::Text;
    }
    if mimetype == "application/pdf" {
        return FileCategory::Pdf;
    }
    if mimetype == "inode/directory" {
        return FileCategory::Directory;
    }
    if is_archive_mimetype(mimetype) {
        return FileCategory::Archive;
    }
    FileCategory::Other
}

fn is_archive_mimetype(mimetype: &str) -> bool {
    matches!(
        mimetype,
        "application/zip"
            | "application/x-tar"
            | "application/gzip"
            | "application/x-gzip"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/x-7z-compressed"
            | "application/x-rar-compressed"
            | "application/vnd.rar"
            | "application/x-compressed-tar"
    )
}

fn detect_mimetype(path: &Path) -> String {
    // Try file --mime-type first (most reliable)
    if let Ok(output) = Command::new("file")
        .args(["--mime-type", "-b"])
        .arg(path)
        .output()
        && output.status.success()
    {
        let mime = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !mime.is_empty() {
            return mime;
        }
    }

    // Fallback to xdg-mime
    if let Ok(output) = Command::new("xdg-mime")
        .args(["query", "filetype"])
        .arg(path)
        .output()
        && output.status.success()
    {
        let mime = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !mime.is_empty() {
            return mime;
        }
    }

    "application/octet-stream".to_string()
}

fn render_media_preview(path: &Path, mimetype: &str, columns: Option<usize>) -> Result<String> {
    let metadata = probe_media_metadata(path);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    let icon = if metadata.is_audio_only() {
        NerdFont::Music
    } else {
        NerdFont::Video
    };

    let mut builder = PreviewBuilder::new().header(icon, file_name);

    if let Some(duration) = metadata.duration_display() {
        builder = builder.field("Duration", &duration);
    }

    if !metadata.is_audio_only() {
        if let Some(resolution) = metadata.resolution_display() {
            builder = builder.field("Resolution", &resolution);
        }
        if let Some(codec) = &metadata.video_codec {
            builder = builder.field("Video Codec", codec);
        }
        if let Some(fps) = metadata.framerate_display() {
            builder = builder.field("Frame Rate", &fps);
        }
    }

    if let Some(codec) = &metadata.audio_codec {
        builder = builder.field("Audio Codec", codec);
    }
    if let Some(channels) = metadata.audio_channels {
        let ch = match channels {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            n => format!("{n} channels"),
        };
        builder = builder.field("Channels", &ch);
    }
    if let Some(bitrate) = metadata.bitrate_display() {
        builder = builder.field("Bitrate", &bitrate);
    }

    builder = append_wrapped_path(builder.blank().field("MIME Type", mimetype), path, columns);

    Ok(builder.build_string())
}

fn render_image_preview(path: &Path, mimetype: &str, columns: Option<usize>) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    let mut builder = PreviewBuilder::new().header(NerdFont::Image, file_name);

    // Try to get image dimensions using file command or identify
    if let Some((width, height)) = probe_image_dimensions(path) {
        builder = builder.field("Dimensions", &format!("{}x{}", width, height));
    }

    if let Ok(meta) = path.metadata() {
        builder = builder.field("Size", &format_size(meta.len()));
        if let Ok(modified) = meta.modified() {
            builder = builder.field("Modified", &format_system_time_for_display(Some(modified)));
        }
    }

    builder = append_wrapped_path(builder.blank().field("MIME Type", mimetype), path, columns);

    Ok(builder.build_string())
}

fn probe_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    // Try file command first
    if let Ok(output) = Command::new("file").arg(path).output()
        && output.status.success()
    {
        let info = String::from_utf8_lossy(&output.stdout);
        // Parse patterns like "1920 x 1080" or "1920x1080"
        if let Some(dims) = parse_dimensions_from_file_output(&info) {
            return Some(dims);
        }
    }

    // Try identify (ImageMagick) if available
    if let Ok(output) = Command::new("identify")
        .args(["-format", "%wx%h"])
        .arg(path)
        .output()
        && output.status.success()
    {
        let dims = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = dims.trim().split('x').collect();
        if parts.len() == 2
            && let (Ok(w), Ok(h)) = (parts[0].parse(), parts[1].parse())
        {
            return Some((w, h));
        }
    }

    None
}

fn parse_dimensions_from_file_output(output: &str) -> Option<(u32, u32)> {
    // Look for patterns like "1920 x 1080" or "1920x1080"
    let re_patterns = [r"(\d+)\s*x\s*(\d+)", r"(\d+)x(\d+)", r", (\d+) x (\d+)"];

    for pattern in re_patterns {
        if let Ok(re) = regex::Regex::new(pattern)
            && let Some(caps) = re.captures(output)
            && let (Some(w), Some(h)) = (caps.get(1), caps.get(2))
            && let (Ok(width), Ok(height)) = (w.as_str().parse::<u32>(), h.as_str().parse::<u32>())
        {
            return Some((width, height));
        }
    }

    None
}

fn render_text_preview(path: &Path, mimetype: &str, columns: Option<usize>) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    let mut builder = PreviewBuilder::new().header(NerdFont::FileText, file_name);

    if let Ok(meta) = path.metadata() {
        builder = builder.field("Size", &format_size(meta.len()));
        if let Ok(modified) = meta.modified() {
            builder = builder.field("Modified", &format_system_time_for_display(Some(modified)));
        }
    }

    // Count lines
    if let Ok(content) = std::fs::read_to_string(path) {
        let lines = content.lines().count();
        builder = builder.field("Lines", &lines.to_string());
    }

    builder = append_wrapped_path(builder.blank().field("MIME Type", mimetype), path, columns);

    Ok(builder.build_string())
}

fn render_archive_preview(path: &Path, mimetype: &str, columns: Option<usize>) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    let mut builder = PreviewBuilder::new().header(NerdFont::Archive, file_name);

    if let Ok(meta) = path.metadata() {
        builder = builder.field("Size", &format_size(meta.len()));
        if let Ok(modified) = meta.modified() {
            builder = builder.field("Modified", &format_system_time_for_display(Some(modified)));
        }
    }

    builder = append_wrapped_path(builder.blank().field("MIME Type", mimetype), path, columns);

    Ok(builder.build_string())
}

fn render_pdf_preview(path: &Path, mimetype: &str, columns: Option<usize>) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    let mut builder = PreviewBuilder::new().header(NerdFont::FilePdf, file_name);

    if let Ok(meta) = path.metadata() {
        builder = builder.field("Size", &format_size(meta.len()));
        if let Ok(modified) = meta.modified() {
            builder = builder.field("Modified", &format_system_time_for_display(Some(modified)));
        }
    }

    // Try pdfinfo if available
    if let Ok(output) = Command::new("pdfinfo").arg(path).output()
        && output.status.success()
    {
        let info = String::from_utf8_lossy(&output.stdout);
        for line in info.lines() {
            if let Some(pages) = line.strip_prefix("Pages:") {
                builder = builder.field("Pages", pages.trim());
            }
            if let Some(title) = line.strip_prefix("Title:") {
                let title = title.trim();
                if !title.is_empty() {
                    builder = builder.field("Title", title);
                }
            }
        }
    }

    builder = append_wrapped_path(builder.blank().field("MIME Type", mimetype), path, columns);

    Ok(builder.build_string())
}

fn render_directory_preview(path: &Path, columns: Option<usize>) -> Result<String> {
    let dir_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Directory");

    let mut builder = PreviewBuilder::new().header(NerdFont::Folder, dir_name);

    if let Ok(entries) = std::fs::read_dir(path) {
        let count = entries.count();
        builder = builder.field("Items", &count.to_string());
    }

    if let Ok(meta) = path.metadata()
        && let Ok(modified) = meta.modified()
    {
        builder = builder.field("Modified", &format_system_time_for_display(Some(modified)));
    }

    builder = append_wrapped_path(builder.blank(), path, columns);

    Ok(builder.build_string())
}

fn render_generic_preview(path: &Path, mimetype: &str, columns: Option<usize>) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    let icon = if path.is_dir() {
        NerdFont::Folder
    } else {
        NerdFont::File
    };

    let mut builder = PreviewBuilder::new().header(icon, file_name);

    if let Ok(meta) = path.metadata() {
        builder = builder
            .field("Type", if meta.is_dir() { "Directory" } else { "File" })
            .field("Size", &format_size(meta.len()));
        if let Ok(modified) = meta.modified() {
            builder = builder.field("Modified", &format_system_time_for_display(Some(modified)));
        }
    }

    builder = append_wrapped_path(builder.blank().field("MIME Type", mimetype), path, columns);

    Ok(builder.build_string())
}

fn append_wrapped_path(
    mut builder: PreviewBuilder,
    path: &Path,
    columns: Option<usize>,
) -> PreviewBuilder {
    let path = path.to_string_lossy();
    let line_width = path_line_width(columns);
    let lines = wrap_path_for_preview(&path, line_width);

    for (index, line) in lines.iter().enumerate() {
        let label = if index == 0 { "Path: " } else { "      " };
        builder = builder.subtext(&format!("{label}{line}"));
    }

    builder
}

fn path_line_width(columns: Option<usize>) -> usize {
    columns.unwrap_or(80).saturating_sub(8).clamp(24, 160)
}

fn wrap_path_for_preview(path: &str, line_width: usize) -> Vec<String> {
    if path.chars().count() <= line_width {
        return vec![path.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();

    for segment in split_path_segments(path) {
        if current.chars().count() + segment.chars().count() <= line_width {
            current.push_str(&segment);
            continue;
        }

        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }

        if segment.chars().count() <= line_width {
            current.push_str(&segment);
        } else {
            lines.extend(split_long_segment(&segment, line_width));
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

fn split_path_segments(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();

    for ch in path.chars() {
        current.push(ch);
        if ch == '/' || ch == '\\' {
            segments.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

fn split_long_segment(segment: &str, line_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for ch in segment.chars() {
        if current.chars().count() >= line_width {
            lines.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_long_paths_at_directory_boundaries() {
        let lines = wrap_path_for_preview(
            "/run/media/benjamin/External Drive/Games/Very Long Game Name/Game.exe",
            24,
        );

        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.chars().count() <= 24));
        assert_eq!(
            lines.join(""),
            "/run/media/benjamin/External Drive/Games/Very Long Game Name/Game.exe"
        );
    }

    #[test]
    fn wraps_single_long_path_component() {
        let lines = wrap_path_for_preview("/mnt/aaaaaaaaaaaaaaaaaaaaaaaaaaaa/Game.exe", 10);

        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.chars().count() <= 10));
        assert_eq!(lines.join(""), "/mnt/aaaaaaaaaaaaaaaaaaaaaaaaaaaa/Game.exe");
    }
}
