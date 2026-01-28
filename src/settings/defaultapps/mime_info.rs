use std::collections::HashMap;

use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::preview::{PreviewId, preview_command};
use crate::ui::prelude::*;

/// Information about a MIME type for display purposes
#[derive(Debug, Clone)]
pub(crate) struct MimeTypeInfo {
    pub mime_type: String,
    pub icon: NerdFont,
    pub description: Option<String>,
}

impl FzfSelectable for MimeTypeInfo {
    fn fzf_display_text(&self) -> String {
        let icon = char::from(self.icon);
        if let Some(desc) = &self.description {
            format!("{} {} - {}", icon, self.mime_type, desc)
        } else {
            format!("{} {}", icon, self.mime_type)
        }
    }

    fn fzf_key(&self) -> String {
        self.mime_type.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Command(preview_command(PreviewId::MimeType))
    }
}

pub(crate) fn get_mime_type_info(mime_type: &str) -> MimeTypeInfo {
    if let Some((icon, desc)) = get_exact_mime_info(mime_type) {
        return MimeTypeInfo {
            mime_type: mime_type.to_string(),
            icon,
            description: Some(desc.to_string()),
        };
    }

    if let Some((prefix, _)) = mime_type.split_once('/') {
        let (icon, desc) = match prefix {
            "image" => (NerdFont::Image, Some("Image Viewer")),
            "video" => (NerdFont::Video, Some("Video Player")),
            "audio" => (NerdFont::Music, Some("Audio Player")),
            "text" => (NerdFont::FileText, Some("Text Editor")),
            "application" => (NerdFont::Package, Some("Application")),
            "inode" => (NerdFont::Folder, Some("File Manager")),
            "x-scheme-handler" => (NerdFont::Link, Some("URL Handler")),
            "message" => (NerdFont::ExternalLink, Some("Email Client")),
            _ => (NerdFont::File, None),
        };

        return MimeTypeInfo {
            mime_type: mime_type.to_string(),
            icon,
            description: desc.map(String::from),
        };
    }

    MimeTypeInfo {
        mime_type: mime_type.to_string(),
        icon: NerdFont::File,
        description: None,
    }
}

pub(crate) fn get_all_mime_types(mime_map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut mime_types: Vec<String> = mime_map.keys().cloned().collect();

    mime_types.sort_by(|a, b| {
        let a_has_exact = has_exact_mime_info(a);
        let b_has_exact = has_exact_mime_info(b);

        match (a_has_exact, b_has_exact) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.cmp(b),
        }
    });

    mime_types
}

fn has_exact_mime_info(mime_type: &str) -> bool {
    get_exact_mime_info(mime_type).is_some()
}

fn get_exact_mime_info(mime_type: &str) -> Option<(NerdFont, &'static str)> {
    let mapping = match mime_type {
        "inode/directory" => (NerdFont::Folder, "File Manager"),
        "text/html" => (NerdFont::Globe, "Web Browser"),
        "x-scheme-handler/http" => (NerdFont::Globe, "Web Browser (HTTP)"),
        "x-scheme-handler/https" => (NerdFont::Globe, "Web Browser (HTTPS)"),
        "x-scheme-handler/mailto" => (NerdFont::ExternalLink, "Email Client"),
        "message/rfc822" => (NerdFont::ExternalLink, "Email Client"),

        "image/jpeg" | "image/jpg" => (NerdFont::Image, "Image Viewer (JPEG)"),
        "image/png" => (NerdFont::Image, "Image Viewer (PNG)"),
        "image/gif" => (NerdFont::Image, "Image Viewer (GIF)"),
        "image/svg+xml" => (NerdFont::Image, "Image Viewer (SVG)"),
        "image/webp" => (NerdFont::Image, "Image Viewer (WebP)"),
        "image/bmp" => (NerdFont::Image, "Image Viewer (BMP)"),
        "image/tiff" => (NerdFont::Image, "Image Viewer (TIFF)"),

        "application/pdf" => (NerdFont::FilePdf, "PDF Viewer"),
        "application/vnd.oasis.opendocument.text" => (NerdFont::FileText, "Document Editor (ODT)"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            (NerdFont::FileWord, "Document Editor (Word)")
        }
        "application/msword" => (NerdFont::FileWord, "Document Editor (Word)"),
        "application/vnd.ms-excel" => (NerdFont::FileExcel, "Spreadsheet Editor (Excel)"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            (NerdFont::FileExcel, "Spreadsheet Editor (Excel)")
        }
        "application/vnd.ms-powerpoint" => (
            NerdFont::FilePresentation,
            "Presentation Editor (PowerPoint)",
        ),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => (
            NerdFont::FilePresentation,
            "Presentation Editor (PowerPoint)",
        ),

        "application/zip" => (NerdFont::Archive, "Archive Manager (ZIP)"),
        "application/x-tar" => (NerdFont::Archive, "Archive Manager (TAR)"),
        "application/x-7z-compressed" => (NerdFont::Archive, "Archive Manager (7-Zip)"),
        "application/x-rar" => (NerdFont::Archive, "Archive Manager (RAR)"),
        "application/gzip" => (NerdFont::Archive, "Archive Manager (GZIP)"),
        "application/x-bzip2" => (NerdFont::Archive, "Archive Manager (BZIP2)"),
        "application/x-xz" => (NerdFont::Archive, "Archive Manager (XZ)"),

        "video/mp4" => (NerdFont::Video, "Video Player (MP4)"),
        "video/x-matroska" => (NerdFont::Video, "Video Player (MKV)"),
        "video/webm" => (NerdFont::Video, "Video Player (WebM)"),
        "video/mpeg" => (NerdFont::Video, "Video Player (MPEG)"),
        "video/x-msvideo" => (NerdFont::Video, "Video Player (AVI)"),

        "audio/mpeg" => (NerdFont::Music, "Audio Player (MP3)"),
        "audio/ogg" => (NerdFont::Music, "Audio Player (OGG)"),
        "audio/flac" => (NerdFont::Music, "Audio Player (FLAC)"),
        "audio/x-wav" => (NerdFont::Music, "Audio Player (WAV)"),
        "audio/aac" => (NerdFont::Music, "Audio Player (AAC)"),

        "text/plain" => (NerdFont::FileText, "Text Editor"),
        "text/css" => (NerdFont::Code, "Code Editor (CSS)"),
        "text/javascript" => (NerdFont::Code, "Code Editor (JavaScript)"),
        "application/json" => (NerdFont::Code, "Code Editor (JSON)"),
        "application/xml" => (NerdFont::Code, "Code Editor (XML)"),
        "text/x-python" => (NerdFont::Code, "Code Editor (Python)"),
        "text/x-rust" => (NerdFont::Code, "Code Editor (Rust)"),
        "text/x-c" => (NerdFont::Code, "Code Editor (C)"),
        "text/x-c++" => (NerdFont::Code, "Code Editor (C++)"),
        "text/markdown" => (NerdFont::FileText, "Text Editor (Markdown)"),

        "application/x-executable" => (NerdFont::Gear, "Executable Program"),
        "application/x-sharedlib" => (NerdFont::Gear, "Shared Library"),
        "application/x-shellscript" => (NerdFont::Terminal, "Terminal (Shell Script)"),

        "application/vnd.appimage" => (NerdFont::Package, "AppImage Launcher"),
        "application/vnd.flatpak.ref" => (NerdFont::Package, "Flatpak Installer"),
        "application/x-iso9660-image" => (NerdFont::Archive, "Disk Image Viewer (ISO)"),

        _ => return None,
    };

    Some(mapping)
}
