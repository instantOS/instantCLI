use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

use crate::common::desktop_entry::DesktopEntry;
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
                .line(
                    colors::GREEN,
                    Some(NerdFont::CheckCircle),
                    "Current default",
                )
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

pub(crate) struct ApplicationInfoCache {
    directories: Vec<PathBuf>,
    entries: HashMap<String, ApplicationInfo>,
}

impl Default for ApplicationInfoCache {
    fn default() -> Self {
        let home_dir = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        Self {
            directories: vec![
                home_dir.join(".local/share/applications"),
                home_dir.join(".local/share/flatpak/exports/share/applications"),
                PathBuf::from("/var/lib/flatpak/exports/share/applications"),
                PathBuf::from("/usr/share/applications"),
            ],
            entries: HashMap::new(),
        }
    }
}

impl ApplicationInfoCache {
    pub(crate) fn get(&mut self, desktop_id: &str) -> ApplicationInfo {
        if let Some(info) = self.entries.get(desktop_id) {
            return info.clone();
        }

        let info = self.load(desktop_id);
        self.entries.insert(desktop_id.to_string(), info.clone());
        info
    }

    fn load(&self, desktop_id: &str) -> ApplicationInfo {
        for directory in &self.directories {
            let path = directory.join(desktop_id);
            if let Ok(content) = std::fs::read_to_string(&path)
                && let Some(entry) = DesktopEntry::parse(&content)
                && entry.entry_type == Some("Application")
            {
                return ApplicationInfo {
                    desktop_id: desktop_id.to_string(),
                    name: entry.name.map(ToOwned::to_owned),
                    comment: entry.comment.map(ToOwned::to_owned),
                    icon: entry.icon.map(ToOwned::to_owned),
                    exec: entry.exec.map(ToOwned::to_owned),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_details_are_parsed_once_per_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("example.desktop");
        std::fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=Original\nComment=Details\nExec=example\n",
        )
        .unwrap();
        let mut cache = ApplicationInfoCache {
            directories: vec![directory.path().to_path_buf()],
            entries: HashMap::new(),
        };

        let first = cache.get("example.desktop");
        std::fs::write(
            path,
            "[Desktop Entry]\nType=Application\nName=Changed\nExec=changed\n",
        )
        .unwrap();
        let second = cache.get("example.desktop");

        assert_eq!(first.name.as_deref(), Some("Original"));
        assert_eq!(first.comment.as_deref(), Some("Details"));
        assert_eq!(second.name.as_deref(), Some("Original"));
    }
}
