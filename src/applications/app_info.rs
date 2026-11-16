use std::collections::HashMap;
use std::path::PathBuf;

use crate::common::desktop_entry::DesktopEntry;
use crate::common::shell::{current_exe_command, shell_quote};
use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::catppuccin::{colors, format_icon_colored, hex_to_ansi_fg};
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
    pub path: Option<PathBuf>,
    pub mime_types: Vec<String>,
}

impl FzfSelectable for ApplicationInfo {
    fn fzf_display_text(&self) -> String {
        self.display_with_badge_color(if self.is_default {
            colors::GREEN
        } else {
            colors::BLUE
        })
    }

    fn fzf_key(&self) -> String {
        self.desktop_id.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        match serde_json::to_string(&ApplicationPreviewKey {
            desktop_id: self.desktop_id.clone(),
            is_default: self.is_default,
        }) {
            Ok(key) => FzfPreview::Command(format!(
                "{} preview --id desktop-application --key {}",
                current_exe_command(),
                shell_quote(&key)
            )),
            Err(error) => {
                FzfPreview::Text(format!("Could not prepare application preview: {error}"))
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ApplicationPreviewKey {
    desktop_id: String,
    is_default: bool,
}

/// Render in fzf's preview process, where the actual pane width is available.
pub(crate) fn render_application_preview(key: &str) -> anyhow::Result<String> {
    let key: ApplicationPreviewKey = serde_json::from_str(key)?;
    let mut app = ApplicationInfoCache::default().get(&key.desktop_id);
    app.is_default = key.is_default;
    Ok(app.preview_text())
}

impl ApplicationInfo {
    /// Let a menu distinguish recommendation groups without duplicating row formatting.
    pub(crate) fn display_with_badge_color(&self, color: &str) -> String {
        let icon = format_icon_colored(NerdFont::Application, color);
        let mut text = if let Some(name) = &self.name {
            format!("{icon} {name} ({})", self.desktop_id)
        } else {
            format!("{icon} {}", self.desktop_id)
        };

        if self.is_default {
            let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
            text.push_str(&format!(" {subtext}(current){RESET}"));
        }

        text
    }

    fn preview_text(&self) -> String {
        let title = self.name.as_deref().unwrap_or(&self.desktop_id);
        let mut builder = PreviewBuilder::new().header(NerdFont::Application, title);

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

        builder.build_string()
    }
}

pub(crate) struct ApplicationInfoCache {
    paths: HashMap<String, PathBuf>,
    entries: HashMap<String, ApplicationInfo>,
}

impl Default for ApplicationInfoCache {
    fn default() -> Self {
        let paths = crate::applications::discovery::discover_file_handlers()
            .into_iter()
            .filter_map(|item| match item {
                crate::applications::types::LaunchItem::DesktopApp { id, path, .. } => {
                    Some((id, path))
                }
                _ => None,
            })
            .collect();
        Self {
            paths,
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

    pub(crate) fn all(&mut self) -> Vec<ApplicationInfo> {
        let ids = self.paths.keys().cloned().collect::<Vec<_>>();
        ids.iter().map(|id| self.get(id)).collect()
    }

    fn load(&self, desktop_id: &str) -> ApplicationInfo {
        if let Some(path) = self.paths.get(desktop_id)
            && let Ok(content) = std::fs::read_to_string(path)
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
                path: Some(path.clone()),
                mime_types: entry
                    .mime_types
                    .unwrap_or_default()
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            };
        }

        ApplicationInfo {
            desktop_id: desktop_id.to_string(),
            name: None,
            comment: None,
            icon: None,
            exec: None,
            is_default: false,
            path: None,
            mime_types: Vec::new(),
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
            paths: HashMap::from([("example.desktop".into(), path.clone())]),
            entries: HashMap::new(),
        };

        let first = cache.get("example.desktop");
        std::fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=Changed\nExec=changed\n",
        )
        .unwrap();
        let second = cache.get("example.desktop");

        assert_eq!(first.name.as_deref(), Some("Original"));
        assert_eq!(first.comment.as_deref(), Some("Details"));
        assert_eq!(second.name.as_deref(), Some("Original"));
    }
}
