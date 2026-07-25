//! Default applications settings
//!
//! Settings for configuring default apps for various file types.

use anyhow::Result;

use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::default_commands::{self, DefaultCommand};
use crate::settings::defaultapps;
use crate::settings::deps::XDG_UTILS;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::colors;
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;

#[derive(Clone)]
struct CommandChoice {
    name: String,
    path: std::path::PathBuf,
    current: bool,
}

#[derive(Clone)]
enum CommandMenuEntry {
    Command(CommandChoice),
    Custom,
}

impl FzfSelectable for CommandMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Command(choice) => {
                let current = if choice.current { " (current)" } else { "" };
                format!("{} {}{}", NerdFont::Terminal, choice.name, current)
            }
            Self::Custom => format!("{} Custom command...", NerdFont::Edit),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Command(choice) => choice.path.display().to_string(),
            Self::Custom => "__custom__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Command(choice) => PreviewBuilder::new()
                .header(NerdFont::Terminal, &choice.name)
                .field("Executable", &choice.path.display().to_string())
                .field(
                    "Default",
                    if choice.current {
                        "Current"
                    } else {
                        "Available"
                    },
                )
                .build(),
            Self::Custom => PreviewBuilder::new()
                .header(NerdFont::Edit, "Custom Command")
                .text("Enter the name or absolute path of an installed executable.")
                .build(),
        }
    }
}

fn select_default_command(ctx: &mut SettingsContext, command: DefaultCommand) -> Result<()> {
    let current = default_commands::current_command(command);
    let choices: Vec<CommandChoice> = default_commands::installed_candidates(command)
        .into_iter()
        .map(|(name, path)| CommandChoice {
            current: current.as_ref() == Some(&path),
            name,
            path,
        })
        .collect();

    let initial_index = choices.iter().position(|choice| choice.current);
    let mut entries: Vec<CommandMenuEntry> =
        choices.into_iter().map(CommandMenuEntry::Command).collect();
    entries.push(CommandMenuEntry::Custom);
    let mut menu = FzfWrapper::builder()
        .prompt(format!("Select {}: ", command.title()))
        .header(
            HeaderBuilder::new(NerdFont::Terminal, format!("Default {}", command.title()))
                .subtitle("Used by instantOS shortcuts and desktop applications")
                .build(),
        )
        .responsive_layout();
    if let Some(index) = initial_index {
        menu = menu.initial_index(index);
    }

    if let FzfResult::Selected(entry) = menu.select(entries)? {
        let (name, path) = match entry {
            CommandMenuEntry::Command(choice) => (choice.name, choice.path),
            CommandMenuEntry::Custom => {
                let input = FzfWrapper::builder()
                    .prompt(format!("{} command", command.title()))
                    .input()
                    .input_dialog()?;
                let input = input.trim();
                if input.is_empty() {
                    return Ok(());
                }
                let path = which::which(input)
                    .map_err(|_| anyhow::anyhow!("command '{input}' was not found in PATH"))?;
                (input.to_string(), path)
            }
        };

        default_commands::set_default_command(command, &path)?;
        ctx.notify(
            &format!("Default {}", command.title()),
            &format!("Set {name} as the default."),
        );
    }
    Ok(())
}

pub struct DefaultTerminal;

impl Setting for DefaultTerminal {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("apps.terminal")
            .title("Terminal Emulator")
            .icon(NerdFont::Terminal)
            .summary("Set the terminal used by instantOS shortcuts and command-line applications.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        select_default_command(ctx, DefaultCommand::Terminal)
    }
}

pub struct DefaultTerminalFileManager;

impl Setting for DefaultTerminalFileManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("apps.terminal_file_manager")
            .title("Terminal File Manager")
            .icon(NerdFont::Folder)
            .summary("Set the command-line file manager opened by Super+R.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        select_default_command(ctx, DefaultCommand::TerminalFileManager)
    }
}

macro_rules! default_app_setting {
    ($struct_name:ident, $id:expr, $title:expr, $icon:expr, $color:expr, $summary:expr, $handler:path) => {
        default_app_setting!(
            $struct_name,
            $id,
            $title,
            $icon,
            $color,
            $summary,
            $handler,
            None
        );
    };
    ($struct_name:ident, $id:expr, $title:expr, $icon:expr, $color:expr, $summary:expr, $handler:path, $preview_id:expr) => {
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

            fn preview_command(&self) -> Option<String> {
                $preview_id.map(|id: PreviewId| preview_command(id))
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
    defaultapps::set_default_browser,
    Some(PreviewId::DefaultBrowser)
);

default_app_setting!(
    DefaultEmail,
    "apps.email",
    "Email Client",
    NerdFont::ExternalLink,
    None,
    "Set your default email client for mailto: links.",
    defaultapps::set_default_email,
    Some(PreviewId::DefaultEmail)
);

default_app_setting!(
    DefaultFileManager,
    "apps.file_manager",
    "File Manager",
    NerdFont::Folder,
    None,
    "Set your default file manager for browsing folders.",
    defaultapps::set_default_file_manager,
    Some(PreviewId::DefaultFileManager)
);

default_app_setting!(
    DefaultTextEditor,
    "apps.text_editor",
    "Text Editor",
    NerdFont::FileText,
    None,
    "Set your default text editor for opening text files.",
    defaultapps::set_default_text_editor,
    Some(PreviewId::DefaultTextEditor)
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
    defaultapps::set_default_pdf_viewer,
    Some(PreviewId::DefaultPdfViewer)
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
