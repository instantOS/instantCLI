//! Default applications settings
//!
//! Settings for configuring default apps for various file types.

use anyhow::Result;

use crate::common::requirements::XDG_UTILS_PACKAGE;
use crate::settings::context::SettingsContext;
use crate::settings::context::colors;
use crate::settings::defaultapps;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

macro_rules! default_app_setting {
    ($struct_name:ident, $id:expr, $title:expr, $icon:expr, $color:expr, $summary:expr, $handler:path) => {
        pub struct $struct_name;

        impl Setting for $struct_name {
            fn metadata(&self) -> SettingMetadata {
                SettingMetadata {
                    id: $id,
                    title: $title,
                    category: Category::Apps,
                    icon: $icon,
                    icon_color: $color,
                    breadcrumbs: &[$title],
                    summary: $summary,
                    requires_reapply: false,
                    requirements: &[Requirement::Package(XDG_UTILS_PACKAGE)],
                }
            }

            fn setting_type(&self) -> SettingType {
                SettingType::Action
            }

            fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
                $handler(ctx)
            }
        }

        inventory::submit! { &$struct_name as &'static dyn Setting }
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

default_app_setting!(
    DefaultImageViewer,
    "apps.image_viewer",
    "Image Viewer",
    NerdFont::Image,
    None,
    "Set your default image viewer for photos and pictures.",
    defaultapps::set_default_image_viewer
);

default_app_setting!(
    DefaultVideoPlayer,
    "apps.video_player",
    "Video Player",
    NerdFont::Video,
    None,
    "Set your default video player for movies and videos.",
    defaultapps::set_default_video_player
);

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
