use std::env;
use std::path::PathBuf;

use freedesktop_file_parser::parse;

use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone)]
pub(crate) struct ApplicationInfo {
    pub desktop_id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub exec: Option<String>,
    pub is_default: bool,
}

impl FzfSelectable for ApplicationInfo {
    fn fzf_display_text(&self) -> String {
        let mut text = if let Some(name) = &self.name {
            format!("󰘔 {} ({})", name, self.desktop_id)
        } else {
            format!("󰘔 {}", self.desktop_id)
        };

        if self.is_default {
            let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
            text.push_str(&format!(" {subtext}(current){RESET}"));
        }

        text
    }

    fn fzf_key(&self) -> String {
        self.desktop_id.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let title = self.name.as_deref().unwrap_or(&self.desktop_id);
        let mut builder = PreviewBuilder::new().header(NerdFont::Desktop, title);

        if self.is_default {
            builder = builder
                .line(colors::GREEN, Some(NerdFont::CheckCircle), "Current default")
                .subtext("Selected for this MIME type.")
                .blank();
        }

        if let Some(comment) = &self.comment {
            builder = builder.subtext(comment).blank();
        }

        builder = builder
            .line(colors::TEAL, Some(NerdFont::ChevronRight), "Details")
            .field_indented("Desktop ID", &self.desktop_id);

        if self.is_default {
            builder = builder.field_indented("Default", "Current");
        }

        if let Some(exec) = &self.exec {
            builder = builder.field_indented("Command", exec);
        }

        if let Some(icon) = &self.icon {
            builder = builder.field_indented("Icon", icon);
        }

        builder.build()
    }
}

pub(crate) fn get_application_info(desktop_id: &str) -> ApplicationInfo {
    let home_dir = env::var("HOME").unwrap_or_default();
    let directories = [
        format!("{home_dir}/.local/share/applications"),
        format!("{home_dir}/.local/share/flatpak/exports/share/applications"),
        "/var/lib/flatpak/exports/share/applications".to_string(),
        "/usr/share/applications".to_string(),
    ];

    for dir in &directories {
        let path = PathBuf::from(dir).join(desktop_id);
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(desktop_file) = parse(&content)
        {
            use freedesktop_file_parser::EntryType;

            let exec = match &desktop_file.entry.entry_type {
                EntryType::Application(app) => app.exec.clone(),
                _ => None,
            };

            return ApplicationInfo {
                desktop_id: desktop_id.to_string(),
                name: Some(desktop_file.entry.name.default.clone()),
                comment: desktop_file
                    .entry
                    .comment
                    .as_ref()
                    .map(|c| c.default.clone()),
                icon: desktop_file.entry.icon.as_ref().map(|i| i.content.clone()),
                exec,
                is_default: false,
            };
        }
    }

    ApplicationInfo {
        desktop_id: desktop_id.to_string(),
        name: None,
        comment: None,
        icon: None,
        exec: None,
        is_default: false,
    }
}
