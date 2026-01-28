//! Default applications settings
//!
//! Settings for configuring default apps for various file types.

use anyhow::Result;

use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::defaultapps;
use crate::settings::deps::XDG_UTILS;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::colors;
use crate::ui::prelude::*;

macro_rules! default_app_setting {
    ($struct_name:ident, $id:expr, $title:expr, $icon:expr, $color:expr, $summary:expr, $handler:path) => {
        pub struct $struct_name;

        impl Setting for $struct_name {
            fn metadata(&self) -> SettingMetadata {
                SettingMetadata::builder()
                    .id($id)
                    .title($title)
                    .icon($icon)
                    .icon_color($color)
                    .summary($summary)
                    .requirements(vec![&XDG_UTILS])
                    .build()
            }

            fn setting_type(&self) -> SettingType {
                SettingType::Action
            }

            fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
                $handler(ctx)
            }
        }
    };
}

default_app_setting!(
    DefaultBrowser,
    "apps.browser",
    "Web Browser",
    NerdFont::Globe,
    None,
    "Set your default web browser for opening links and HTML files.",
    defaultapps::set_default_browser
);

default_app_setting!(
    DefaultEmail,
    "apps.email",
    "Email Client",
    NerdFont::ExternalLink,
    None,
    "Set your default email client for mailto: links.",
    defaultapps::set_default_email
);

default_app_setting!(
    DefaultFileManager,
    "apps.file_manager",
    "File Manager",
    NerdFont::Folder,
    None,
    "Set your default file manager for browsing folders.",
    defaultapps::set_default_file_manager
);

default_app_setting!(
    DefaultTextEditor,
    "apps.text_editor",
    "Text Editor",
    NerdFont::FileText,
    None,
    "Set your default text editor for opening text files.",
    defaultapps::set_default_text_editor
);

pub struct DefaultImageViewer;

impl Setting for DefaultImageViewer {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("apps.image_viewer")
            .title("Image Viewer")
            .icon(NerdFont::Image)
            .icon_color(None)
            .summary("Set your default image viewer for photos and pictures.")
            .requirements(vec![&XDG_UTILS])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        defaultapps::set_default_image_viewer(ctx)
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::DefaultImageViewer))
    }
}

pub struct DefaultVideoPlayer;

impl Setting for DefaultVideoPlayer {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("apps.video_player")
            .title("Video Player")
            .icon(NerdFont::Video)
            .icon_color(None)
            .summary("Set your default video player for movies and videos.")
            .requirements(vec![&XDG_UTILS])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        defaultapps::set_default_video_player(ctx)
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::DefaultVideoPlayer))
    }
}

pub struct DefaultAudioPlayer;

impl Setting for DefaultAudioPlayer {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("apps.audio_player")
            .title("Audio Player")
            .icon(NerdFont::Music)
            .icon_color(None)
            .summary("Set your default audio player for music and podcasts.")
            .requirements(vec![&XDG_UTILS])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        defaultapps::set_default_audio_player(ctx)
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::DefaultAudioPlayer))
    }
}

default_app_setting!(
    DefaultPdfViewer,
    "apps.pdf_viewer",
    "PDF Viewer",
    NerdFont::FilePdf,
    None,
    "Set your default PDF viewer for documents.",
    defaultapps::set_default_pdf_viewer
);

pub struct DefaultArchiveManager;

impl Setting for DefaultArchiveManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("apps.archive_manager")
            .title("Archive Manager")
            .icon(NerdFont::Archive)
            .icon_color(None)
            .summary("Set your default archive manager for ZIP, TAR, and other compressed files.")
            .requirements(vec![&XDG_UTILS])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        defaultapps::set_default_archive_manager(ctx)
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::DefaultArchiveManager))
    }
}

default_app_setting!(
    ManageAllApps,
    "apps.default",
    "All File Types",
    NerdFont::Link,
    Some(colors::YELLOW),
    "Advanced: Manage default applications for all file types and MIME types.",
    defaultapps::manage_default_apps
);
