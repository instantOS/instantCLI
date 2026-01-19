//! Default applications settings
//!
//! Settings for configuring default apps for various file types.

use anyhow::Result;

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
        Some(
            r#"bash -c '
echo "Set your default image viewer for photos and pictures."
echo ""
echo "MIME types that will be configured:"
echo "  • image/png"
echo "  • image/jpeg"
echo "  • image/gif"
echo "  • image/webp"
echo "  • image/bmp"
echo "  • image/tiff"
echo "  • image/svg+xml"
echo ""
echo "Only applications that support ALL these formats will be shown."
echo ""
echo "Current defaults:"
for mime in image/png image/jpeg image/gif image/webp image/bmp image/tiff image/svg+xml; do
    app=$(xdg-mime query default "$mime" 2>/dev/null)
    if [ -n "$app" ]; then
        # Try to get app name from desktop file
        name=""
        for dir in "$HOME/.local/share/applications" "/usr/share/applications" "/var/lib/flatpak/exports/share/applications"; do
            if [ -f "$dir/$app" ]; then
                name=$(grep "^Name=" "$dir/$app" 2>/dev/null | head -1 | cut -d= -f2)
                break
            fi
        done
        if [ -n "$name" ]; then
            echo "  $mime: $name"
        else
            echo "  $mime: $app"
        fi
    else
        echo "  $mime: (not set)"
    fi
done
'"#
            .to_string(),
        )
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
        Some(
            r#"bash -c '
echo "Set your default video player for movies and videos."
echo ""
echo "MIME types that will be configured:"
echo "  • video/mp4 (MP4)"
echo "  • video/x-matroska (MKV)"
echo "  • video/webm (WebM)"
echo "  • video/quicktime (MOV)"
echo "  • video/x-msvideo (AVI)"
echo "  • video/ogg (OGG)"
echo ""
echo "Only applications that support ALL these formats will be shown."
echo ""
echo "Current defaults:"
for mime in video/mp4 video/x-matroska video/webm video/quicktime video/x-msvideo video/ogg; do
    app=$(xdg-mime query default "$mime" 2>/dev/null)
    if [ -n "$app" ]; then
        name=""
        for dir in "$HOME/.local/share/applications" "/usr/share/applications" "/var/lib/flatpak/exports/share/applications"; do
            if [ -f "$dir/$app" ]; then
                name=$(grep "^Name=" "$dir/$app" 2>/dev/null | head -1 | cut -d= -f2)
                break
            fi
        done
        if [ -n "$name" ]; then
            echo "  $mime: $name"
        else
            echo "  $mime: $app"
        fi
    else
        echo "  $mime: (not set)"
    fi
done
'"#
            .to_string(),
        )
    }
}

default_app_setting!(
    DefaultMusicPlayer,
    "apps.music_player",
    "Music Player",
    NerdFont::Music,
    None,
    "Set your default music player for audio files.",
    defaultapps::set_default_music_player
);

default_app_setting!(
    DefaultPdfViewer,
    "apps.pdf_viewer",
    "PDF Viewer",
    NerdFont::FilePdf,
    None,
    "Set your default PDF viewer for documents.",
    defaultapps::set_default_pdf_viewer
);

default_app_setting!(
    DefaultArchiveManager,
    "apps.archive_manager",
    "Archive Manager",
    NerdFont::Archive,
    None,
    "Set your default archive manager for ZIP, TAR, and other compressed files.",
    defaultapps::set_default_archive_manager
);

default_app_setting!(
    ManageAllApps,
    "apps.default",
    "All File Types",
    NerdFont::Link,
    Some(colors::YELLOW),
    "Advanced: Manage default applications for all file types and MIME types.",
    defaultapps::manage_default_apps
);
