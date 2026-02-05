//! File suggestion preview - generates rich previews based on file type/mimetype.

use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::arch::dualboot::types::format_size;
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

    if !path.exists() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Warning, "File not found")
            .subtext(path_str)
            .build_string());
    }

    let mimetype = detect_mimetype(path);
    let category = mimetype_category(&mimetype);

    match category {
        FileCategory::Video | FileCategory::Audio => render_media_preview(path, &mimetype),
        FileCategory::Image => render_image_preview(path, &mimetype),
        FileCategory::Text => render_text_preview(path, &mimetype),
        FileCategory::Archive => render_archive_preview(path, &mimetype),
        FileCategory::Pdf => render_pdf_preview(path, &mimetype),
        FileCategory::Directory => render_directory_preview(path),
        FileCategory::Other => render_generic_preview(path, &mimetype),
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
    {
        if output.status.success() {
            let mime = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !mime.is_empty() {
                return mime;
            }
        }
    }

    // Fallback to xdg-mime
    if let Ok(output) = Command::new("xdg-mime")
        .args(["query", "filetype"])
        .arg(path)
        .output()
    {
        if output.status.success() {
            let mime = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !mime.is_empty() {
                return mime;
            }
        }
    }

    "application/octet-stream".to_string()
}

fn render_media_preview(path: &Path, mimetype: &str) -> Result<String> {
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

    builder = builder
        .blank()
        .field("MIME Type", mimetype)
        .subtext(&path.to_string_lossy());

    Ok(builder.build_string())
}

fn render_image_preview(path: &Path, mimetype: &str) -> Result<String> {
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

    builder = builder
        .blank()
        .field("MIME Type", mimetype)
        .subtext(&path.to_string_lossy());

    Ok(builder.build_string())
}

fn probe_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    // Try file command first
    if let Ok(output) = Command::new("file").arg(path).output() {
        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            // Parse patterns like "1920 x 1080" or "1920x1080"
            if let Some(dims) = parse_dimensions_from_file_output(&info) {
                return Some(dims);
            }
        }
    }

    // Try identify (ImageMagick) if available
    if let Ok(output) = Command::new("identify")
        .args(["-format", "%wx%h"])
        .arg(path)
        .output()
    {
        if output.status.success() {
            let dims = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = dims.trim().split('x').collect();
            if parts.len() == 2 {
                if let (Ok(w), Ok(h)) = (parts[0].parse(), parts[1].parse()) {
                    return Some((w, h));
                }
            }
        }
    }

    None
}

fn parse_dimensions_from_file_output(output: &str) -> Option<(u32, u32)> {
    // Look for patterns like "1920 x 1080" or "1920x1080"
    let re_patterns = [
        r"(\d+)\s*x\s*(\d+)",
        r"(\d+)x(\d+)",
        r", (\d+) x (\d+)",
    ];

    for pattern in re_patterns {
        if let Some(caps) = regex_lite::Regex::new(pattern)
            .ok()
            .and_then(|re| re.captures(output))
        {
            if let (Some(w), Some(h)) = (caps.get(1), caps.get(2)) {
                if let (Ok(width), Ok(height)) = (w.as_str().parse(), h.as_str().parse()) {
                    return Some((width, height));
                }
            }
        }
    }

    None
}

fn render_text_preview(path: &Path, mimetype: &str) -> Result<String> {
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

    builder = builder
        .blank()
        .field("MIME Type", mimetype)
        .subtext(&path.to_string_lossy());

    Ok(builder.build_string())
}

fn render_archive_preview(path: &Path, mimetype: &str) -> Result<String> {
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

    builder = builder
        .blank()
        .field("MIME Type", mimetype)
        .subtext(&path.to_string_lossy());

    Ok(builder.build_string())
}

fn render_pdf_preview(path: &Path, mimetype: &str) -> Result<String> {
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
    if let Ok(output) = Command::new("pdfinfo").arg(path).output() {
        if output.status.success() {
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
    }

    builder = builder
        .blank()
        .field("MIME Type", mimetype)
        .subtext(&path.to_string_lossy());

    Ok(builder.build_string())
}

fn render_directory_preview(path: &Path) -> Result<String> {
    let dir_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Directory");

    let mut builder = PreviewBuilder::new().header(NerdFont::Folder, dir_name);

    if let Ok(entries) = std::fs::read_dir(path) {
        let count = entries.count();
        builder = builder.field("Items", &count.to_string());
    }

    if let Ok(meta) = path.metadata() {
        if let Ok(modified) = meta.modified() {
            builder = builder.field("Modified", &format_system_time_for_display(Some(modified)));
        }
    }

    builder = builder.blank().subtext(&path.to_string_lossy());

    Ok(builder.build_string())
}

fn render_generic_preview(path: &Path, mimetype: &str) -> Result<String> {
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

    builder = builder
        .blank()
        .field("MIME Type", mimetype)
        .subtext(&path.to_string_lossy());

    Ok(builder.build_string())
}
