use anyhow::Result;
use clap::ValueEnum;
use std::env;

use crate::common::shell::shell_quote;
use crate::settings::defaultapps::{
    ARCHIVE_MIME_TYPES, AUDIO_MIME_TYPES, IMAGE_MIME_TYPES, VIDEO_MIME_TYPES,
};

const BROWSER_MIME_TYPES: &[&str] = &["text/html"];
const TEXT_EDITOR_MIME_TYPES: &[&str] = &["text/plain"];
const EMAIL_MIME_TYPES: &[&str] = &["x-scheme-handler/mailto"];
const FILE_MANAGER_MIME_TYPES: &[&str] = &["inode/directory"];
const PDF_VIEWER_MIME_TYPES: &[&str] = &["application/pdf"];
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

mod appearance;
mod bluetooth;
mod default_apps;
mod disks;
mod file;
mod helpers;
mod keyboard;
mod mime;
mod mouse;
mod timezone;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PreviewId {
    #[value(name = "keyboard-layout")]
    KeyboardLayout,
    #[value(name = "tty-keymap")]
    TtyKeymap,
    #[value(name = "login-screen-layout")]
    LoginScreenLayout,
    #[value(name = "timezone")]
    Timezone,
    #[value(name = "mime-type")]
    MimeType,
    #[value(name = "bluetooth")]
    Bluetooth,
    #[value(name = "dark-mode")]
    DarkMode,
    #[value(name = "gtk-theme")]
    GtkTheme,
    #[value(name = "icon-theme")]
    IconTheme,
    #[value(name = "cursor-theme")]
    CursorTheme,
    #[value(name = "mouse-sensitivity")]
    MouseSensitivity,
    #[value(name = "default-image-viewer")]
    DefaultImageViewer,
    #[value(name = "default-video-player")]
    DefaultVideoPlayer,
    #[value(name = "default-audio-player")]
    DefaultAudioPlayer,
    #[value(name = "default-archive-manager")]
    DefaultArchiveManager,
    #[value(name = "default-browser")]
    DefaultBrowser,
    #[value(name = "default-text-editor")]
    DefaultTextEditor,
    #[value(name = "default-email")]
    DefaultEmail,
    #[value(name = "default-file-manager")]
    DefaultFileManager,
    #[value(name = "default-pdf-viewer")]
    DefaultPdfViewer,
    #[value(name = "disk")]
    Disk,
    #[value(name = "partition")]
    Partition,
    #[value(name = "file-suggestion")]
    FileSuggestion,
}

impl PreviewId {
    pub fn as_str(&self) -> &'static str {
        match self {
            PreviewId::KeyboardLayout => "keyboard-layout",
            PreviewId::TtyKeymap => "tty-keymap",
            PreviewId::LoginScreenLayout => "login-screen-layout",
            PreviewId::Timezone => "timezone",
            PreviewId::MimeType => "mime-type",
            PreviewId::Bluetooth => "bluetooth",
            PreviewId::DarkMode => "dark-mode",
            PreviewId::GtkTheme => "gtk-theme",
            PreviewId::IconTheme => "icon-theme",
            PreviewId::CursorTheme => "cursor-theme",
            PreviewId::MouseSensitivity => "mouse-sensitivity",
            PreviewId::DefaultImageViewer => "default-image-viewer",
            PreviewId::DefaultVideoPlayer => "default-video-player",
            PreviewId::DefaultAudioPlayer => "default-audio-player",
            PreviewId::DefaultArchiveManager => "default-archive-manager",
            PreviewId::DefaultBrowser => "default-browser",
            PreviewId::DefaultTextEditor => "default-text-editor",
            PreviewId::DefaultEmail => "default-email",
            PreviewId::DefaultFileManager => "default-file-manager",
            PreviewId::DefaultPdfViewer => "default-pdf-viewer",
            PreviewId::Disk => "disk",
            PreviewId::Partition => "partition",
            PreviewId::FileSuggestion => "file-suggestion",
        }
    }
}

impl std::fmt::Display for PreviewId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn preview_command(id: PreviewId) -> String {
    let exe = current_exe_command();
    format!("{exe} preview --id {} --key \"$1\"", id.as_str())
}

pub fn handle_preview_command(id: PreviewId, key: Option<String>) -> Result<()> {
    let ctx = PreviewContext {
        key,
        columns: env_usize("FZF_PREVIEW_COLUMNS"),
        lines: env_usize("FZF_PREVIEW_LINES"),
    };

    let output = match render_preview(id, &ctx) {
        Ok(text) => text,
        Err(err) => render_error_preview(id, err),
    };

    print!("{output}");
    Ok(())
}

pub(crate) struct PreviewContext {
    key: Option<String>,
    #[allow(dead_code)]
    columns: Option<usize>,
    #[allow(dead_code)]
    lines: Option<usize>,
}

impl PreviewContext {
    pub(crate) fn key(&self) -> Option<&str> {
        self.key.as_deref().filter(|k| !k.is_empty())
    }
}

fn render_preview(id: PreviewId, ctx: &PreviewContext) -> Result<String> {
    match id {
        PreviewId::KeyboardLayout => keyboard::render_keyboard_layout_preview(),
        PreviewId::TtyKeymap => keyboard::render_tty_keymap_preview(),
        PreviewId::LoginScreenLayout => keyboard::render_login_screen_layout_preview(),
        PreviewId::Timezone => timezone::render_timezone_preview(ctx),
        PreviewId::MimeType => mime::render_mime_type_preview(ctx),
        PreviewId::Bluetooth => bluetooth::render_bluetooth_preview(),
        PreviewId::DarkMode => appearance::render_dark_mode_preview(),
        PreviewId::GtkTheme => appearance::render_gtk_theme_preview(),
        PreviewId::IconTheme => appearance::render_icon_theme_preview(),
        PreviewId::CursorTheme => appearance::render_cursor_theme_preview(),
        PreviewId::MouseSensitivity => mouse::render_mouse_sensitivity_preview(),
        PreviewId::DefaultImageViewer => default_apps::render_default_app_preview(
            "Image Viewer",
            NerdFont::Image,
            "Set your default image viewer for photos and pictures.",
            IMAGE_MIME_TYPES,
        ),
        PreviewId::DefaultVideoPlayer => default_apps::render_default_app_preview(
            "Video Player",
            NerdFont::Video,
            "Set your default video player for movies and videos.",
            VIDEO_MIME_TYPES,
        ),
        PreviewId::DefaultAudioPlayer => default_apps::render_default_app_preview(
            "Audio Player",
            NerdFont::Music,
            "Set your default audio player for music and podcasts.",
            AUDIO_MIME_TYPES,
        ),
        PreviewId::DefaultArchiveManager => default_apps::render_default_app_preview(
            "Archive Manager",
            NerdFont::Archive,
            "Set your default archive manager for ZIP, TAR, and other compressed files.",
            ARCHIVE_MIME_TYPES,
        ),
        PreviewId::DefaultBrowser => default_apps::render_default_app_preview(
            "Web Browser",
            NerdFont::Globe,
            "Set your default web browser for opening links and HTML files.",
            BROWSER_MIME_TYPES,
        ),
        PreviewId::DefaultTextEditor => default_apps::render_default_app_preview(
            "Text Editor",
            NerdFont::FileText,
            "Set your default text editor for opening text files.",
            TEXT_EDITOR_MIME_TYPES,
        ),
        PreviewId::DefaultEmail => default_apps::render_default_app_preview(
            "Email Client",
            NerdFont::ExternalLink,
            "Set your default email client for mailto: links.",
            EMAIL_MIME_TYPES,
        ),
        PreviewId::DefaultFileManager => default_apps::render_default_app_preview(
            "File Manager",
            NerdFont::Folder,
            "Set your default file manager for browsing folders.",
            FILE_MANAGER_MIME_TYPES,
        ),
        PreviewId::DefaultPdfViewer => default_apps::render_default_app_preview(
            "PDF Viewer",
            NerdFont::FilePdf,
            "Set your default PDF viewer for documents.",
            PDF_VIEWER_MIME_TYPES,
        ),
        PreviewId::Disk => disks::render_disk_preview(ctx),
        PreviewId::Partition => disks::render_partition_preview(ctx),
        PreviewId::FileSuggestion => file::render_file_suggestion_preview(ctx),
    }
}

fn render_error_preview(id: PreviewId, err: anyhow::Error) -> String {
    PreviewBuilder::new()
        .header(NerdFont::Warning, "Preview Unavailable")
        .subtext(&format!("Failed to render '{id}'"))
        .blank()
        .text(&err.to_string())
        .build_string()
}

fn current_exe_command() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "ins".to_string());
    shell_quote(&exe)
}

fn env_usize(name: &str) -> Option<usize> {
    env::var(name).ok().and_then(|v| v.parse::<usize>().ok())
}
