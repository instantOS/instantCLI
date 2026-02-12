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
mod package;
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
    #[value(name = "package")]
    Package,
    #[value(name = "installed-package")]
    InstalledPackage,
    #[value(name = "apt")]
    Apt,
    #[value(name = "dnf")]
    Dnf,
    #[value(name = "zypper")]
    Zypper,
    #[value(name = "pacman")]
    Pacman,
    #[value(name = "snap")]
    Snap,
    #[value(name = "pkg")]
    Pkg,
    #[value(name = "flatpak")]
    Flatpak,
    #[value(name = "aur")]
    Aur,
    #[value(name = "cargo")]
    Cargo,
    #[value(name = "setting")]
    Setting,
    #[value(name = "systemd-service")]
    SystemdService,
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
            PreviewId::Package => "package",
            PreviewId::InstalledPackage => "installed-package",
            PreviewId::Apt => "apt",
            PreviewId::Dnf => "dnf",
            PreviewId::Zypper => "zypper",
            PreviewId::Pacman => "pacman",
            PreviewId::Snap => "snap",
            PreviewId::Pkg => "pkg",
            PreviewId::Flatpak => "flatpak",
            PreviewId::Aur => "aur",
            PreviewId::Cargo => "cargo",
            PreviewId::Setting => "setting",
            PreviewId::SystemdService => "systemd-service",
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

/// Preview command for streaming fzf menus.
/// Uses fzf's {} placeholder instead of $1 for the key.
pub fn preview_command_streaming(id: PreviewId) -> String {
    let exe = current_exe_command();
    format!("{exe} preview --id {} --key {{}}", id.as_str())
}

/// Preview command for a specific setting by ID.
pub fn preview_command_for_setting(setting_id: &str) -> String {
    let exe = current_exe_command();
    format!(
        "{exe} preview --id setting --key {}",
        shell_quote(setting_id)
    )
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
        PreviewId::Package => package::render_package_preview(ctx),
        PreviewId::InstalledPackage => package::render_installed_package_preview(ctx),
        PreviewId::Apt => package::render_apt_preview(ctx),
        PreviewId::Dnf => package::render_dnf_preview(ctx),
        PreviewId::Zypper => package::render_zypper_preview(ctx),
        PreviewId::Pacman => package::render_pacman_preview(ctx),
        PreviewId::Snap => package::render_snap_preview(ctx),
        PreviewId::Pkg => package::render_pkg_preview(ctx),
        PreviewId::Flatpak => package::render_flatpak_preview(ctx),
        PreviewId::Aur => package::render_aur_preview(ctx),
        PreviewId::Cargo => package::render_cargo_preview(ctx),
        PreviewId::Setting => render_setting_preview(ctx),
        PreviewId::SystemdService => render_systemd_service_preview(ctx),
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

fn render_setting_preview(ctx: &PreviewContext) -> Result<String> {
    use crate::settings::setting::{SettingState, setting_by_id};
    use crate::ui::catppuccin::colors;

    let setting_id = ctx
        .key()
        .ok_or_else(|| anyhow::anyhow!("No setting ID provided"))?;
    let setting = setting_by_id(setting_id)
        .ok_or_else(|| anyhow::anyhow!("Setting '{}' not found", setting_id))?;

    let meta = setting.metadata();
    let category = crate::settings::category_tree::get_category_for_setting(meta.id)
        .unwrap_or(crate::settings::setting::Category::System);
    let icon_color = meta.icon_color.unwrap_or_else(|| category.meta().color);

    let mut builder = PreviewBuilder::new()
        .line(icon_color, Some(meta.icon), meta.title)
        .separator()
        .blank();

    if let Some(cmd) = setting.preview_command() {
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(&cmd)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_else(|_| format!("Failed to run preview command: {}", cmd));
        builder = builder.text(&output);
    } else {
        match crate::settings::store::SettingsStore::load() {
            Ok(store) => {
                let settings_ctx =
                    crate::settings::context::SettingsContext::new(store, false, false);
                let state = setting.get_display_state(&settings_ctx);

                match state {
                    SettingState::Toggle { enabled: true } => {
                        builder = builder
                            .line(colors::GREEN, Some(NerdFont::Check), "Enabled")
                            .blank();
                    }
                    SettingState::Toggle { enabled: false } => {
                        builder = builder
                            .line(colors::RED, Some(NerdFont::Cross), "Disabled")
                            .blank();
                    }
                    SettingState::Choice { current_label } => {
                        builder = builder.field("Current", &current_label).blank();
                    }
                    SettingState::Action | SettingState::Command => {}
                }
            }
            Err(_) => {
                builder = builder.subtext("(Could not load settings store)").blank();
            }
        }

        for line in meta.summary.lines() {
            builder = builder.text(line);
        }
    }

    Ok(builder.build_string())
}

fn render_systemd_service_preview(ctx: &PreviewContext) -> Result<String> {
    use crate::ui::catppuccin::colors;

    let key = ctx
        .key()
        .ok_or_else(|| anyhow::anyhow!("No service name provided"))?;

    let parts: Vec<&str> = key.splitn(2, ':').collect();
    let service_name = parts[0];
    let scope = parts.get(1).copied().unwrap_or("system");

    let scope_args: Vec<&str> = if scope == "user" {
        vec!["--user"]
    } else {
        vec![]
    };

    let active = std::process::Command::new("systemctl")
        .args(["is-active", service_name])
        .args(&scope_args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let enabled = std::process::Command::new("systemctl")
        .args(["is-enabled", service_name])
        .args(&scope_args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let description = std::process::Command::new("systemctl")
        .args(["show", service_name, "-p", "Description", "--value"])
        .args(&scope_args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| String::new());

    let active_color = match active.as_str() {
        "active" => colors::GREEN,
        "failed" => colors::RED,
        "inactive" => colors::OVERLAY0,
        _ => colors::YELLOW,
    };

    let enabled_color = match enabled.as_str() {
        "enabled" => colors::GREEN,
        "disabled" => colors::OVERLAY0,
        _ => colors::SUBTEXT0,
    };

    let scope_label = if scope == "user" { "User" } else { "System" };

    let builder = PreviewBuilder::new()
        .header(NerdFont::Server, service_name)
        .field("Description", &description)
        .blank()
        .line(
            active_color,
            Some(NerdFont::CheckCircle),
            &format!("Status: {}", active),
        )
        .line(
            enabled_color,
            Some(NerdFont::ToggleOn),
            &format!("Enabled: {}", enabled),
        )
        .field("Scope", scope_label)
        .blank()
        .separator()
        .blank()
        .text("Actions:")
        .bullet("Start/Stop/Restart the service")
        .bullet("Enable/Disable at boot")
        .bullet("View live logs (journalctl -f)")
        .blank()
        .separator()
        .blank()
        .text("Run 'journalctl -u SERVICE -f' to view live logs");

    Ok(builder.build_string())
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
