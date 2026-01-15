//! FZF picker types and selection logic for alternatives.

use crate::dot::override_config::DotfileSource;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::discovery::DiscoveredDotfile;

/// A source option in the picker.
#[derive(Clone)]
pub struct SourceOption {
    pub source: DotfileSource,
    pub is_current: bool,
    pub exists: bool,
}

impl FzfSelectable for SourceOption {
    fn fzf_display_text(&self) -> String {
        let current = if self.is_current { " (current)" } else { "" };
        let status = if self.exists { "" } else { " [new]" };
        format!(
            "{} / {}{}{}",
            self.source.repo_name, self.source.subdir_name, current, status
        )
    }

    fn fzf_key(&self) -> String {
        format!("{}:{}", self.source.repo_name, self.source.subdir_name)
    }
}

/// Menu item for alternative selection.
#[derive(Clone)]
pub enum MenuItem {
    Source(SourceOption),
    RemoveOverride { default_source: DotfileSource },
    Back,
    Cancel,
}

impl FzfSelectable for MenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            MenuItem::Source(item) => {
                let current = if item.is_current { " (current)" } else { "" };
                let status = if item.exists { "" } else { " [new]" };
                format!(
                    "{} {} / {}{}{}",
                    format_icon_colored(NerdFont::Folder, colors::MAUVE),
                    item.source.repo_name,
                    item.source.subdir_name,
                    current,
                    status
                )
            }
            MenuItem::RemoveOverride { default_source } => {
                format!(
                    "{} Remove Override -> {} / {}",
                    format_icon_colored(NerdFont::Trash, colors::RED),
                    default_source.repo_name,
                    default_source.subdir_name
                )
            }
            MenuItem::Back => format!("{} Back", format_back_icon()),
            MenuItem::Cancel => {
                format!(
                    "{} Cancel",
                    format_icon_colored(NerdFont::Cross, colors::OVERLAY0)
                )
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            MenuItem::Source(item) => {
                format!("{}:{}", item.source.repo_name, item.source.subdir_name)
            }
            MenuItem::RemoveOverride { .. } => "!__remove_override__".to_string(),
            MenuItem::Back | MenuItem::Cancel => "!__back__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            MenuItem::Source(item) => {
                let mut b = PreviewBuilder::new().header(
                    NerdFont::Folder,
                    &format!("{} / {}", item.source.repo_name, item.source.subdir_name),
                );
                if item.is_current {
                    b = b.blank().line(
                        colors::GREEN,
                        Some(NerdFont::Check),
                        "Currently selected source",
                    );
                }
                if !item.exists {
                    b = b.blank().line(
                        colors::YELLOW,
                        Some(NerdFont::Plus),
                        "File will be created in this location",
                    );
                }
                b = b.blank().line(
                    colors::TEXT,
                    Some(NerdFont::File),
                    &format!("Path: {}", item.source.source_path.display()),
                );
                FzfPreview::Text(b.build_string())
            }
            MenuItem::RemoveOverride { default_source } => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Trash, "Remove Override")
                    .blank()
                    .text("Remove the manual override for this file.")
                    .blank()
                    .line(
                        colors::PEACH,
                        Some(NerdFont::ArrowRight),
                        "After removal, the file will be sourced from:",
                    )
                    .indented_line(
                        colors::GREEN,
                        None,
                        &format!(
                            "{} / {}",
                            default_source.repo_name, default_source.subdir_name
                        ),
                    )
                    .blank()
                    .text("This is the default based on repository priority.")
                    .build_string(),
            ),
            MenuItem::Back => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::ArrowLeft, "Back")
                    .blank()
                    .text("Return to previous menu.")
                    .build_string(),
            ),
            MenuItem::Cancel => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Cross, "Cancel")
                    .blank()
                    .text("Exit without making changes.")
                    .build_string(),
            ),
        }
    }
}

impl FzfSelectable for DiscoveredDotfile {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} {} ({} sources)",
            format_icon_colored(NerdFont::File, colors::SKY),
            self.display_path,
            self.sources.len()
        )
    }

    fn fzf_key(&self) -> String {
        self.display_path.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut b = PreviewBuilder::new()
            .header(NerdFont::File, &self.display_path)
            .blank()
            .line(colors::MAUVE, Some(NerdFont::List), "Available sources:");

        for (i, source) in self.sources.iter().enumerate() {
            b = b.indented_line(
                colors::TEXT,
                None,
                &format!("{}. {} / {}", i + 1, source.repo_name, source.subdir_name),
            );
        }
        FzfPreview::Text(b.build_string())
    }
}
