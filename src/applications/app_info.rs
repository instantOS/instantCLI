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
    roots: Vec<PathBuf>,
    path_dirs: Vec<PathBuf>,
    paths: Option<HashMap<String, PathBuf>>,
    entries: HashMap<String, ApplicationInfo>,
}

impl Default for ApplicationInfoCache {
    fn default() -> Self {
        Self::new(
            crate::applications::data_dirs()
                .into_iter()
                .map(|root| root.join("applications"))
                .collect(),
        )
    }
}

impl ApplicationInfoCache {
    pub(crate) fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            path_dirs: std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .collect(),
            paths: None,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn get(&mut self, desktop_id: &str) -> ApplicationInfo {
        if let Some(info) = self.entries.get(desktop_id) {
            return info.clone();
        }

        let info = self.load(desktop_id);
        self.entries.insert(desktop_id.to_string(), info.clone());
        info
    }

    pub(crate) fn all(&mut self) -> Vec<ApplicationInfo> {
        let paths = self
            .paths
            .get_or_insert_with(|| crate::applications::discovery::desktop_paths(&self.roots));
        let ids = paths.keys().cloned().collect::<Vec<_>>();
        ids.iter()
            .map(|id| self.get(id))
            .filter(|app| app.path.is_some())
            .collect()
    }

    fn load(&self, desktop_id: &str) -> ApplicationInfo {
        let path = match &self.paths {
            Some(paths) => paths.get(desktop_id).cloned(),
            None => crate::applications::discovery::desktop_path(&self.roots, desktop_id),
        };
        if let Some(path) = path
            && path.is_file()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Some(entry) = DesktopEntry::parse(&content)
            && crate::applications::discovery::handler_available(&entry, &self.path_dirs, None)
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
    fn direct_lookup_and_catalog_agree_on_masks_nested_ids_and_availability() {
        let root = tempfile::tempdir().unwrap();
        let high = root.path().join("high");
        let low = root.path().join("low");
        std::fs::create_dir_all(high.join("vendor")).unwrap();
        std::fs::create_dir_all(&low).unwrap();
        let entry = "[Desktop Entry]\nType=Application\nName=Example\nExec=example %f\n";
        std::fs::write(high.join("vendor/example.desktop"), entry).unwrap();
        std::fs::write(
            high.join("vendor-example.desktop"),
            entry.replace("Example", "Flat"),
        )
        .unwrap();
        std::fs::write(high.join("hidden.desktop"), format!("{entry}Hidden=true\n")).unwrap();
        std::fs::write(low.join("hidden.desktop"), entry).unwrap();
        std::fs::write(
            high.join("missing.desktop"),
            format!("{entry}TryExec=/nonexistent/ins-test-command\n"),
        )
        .unwrap();
        std::fs::write(
            high.join("handler.desktop"),
            format!("{entry}NoDisplay=true\nOnlyShowIn=OtherDesktop;\n"),
        )
        .unwrap();
        std::os::unix::fs::symlink(high.join("vendor"), high.join("linked.desktop")).unwrap();
        std::fs::write(low.join("linked.desktop"), entry).unwrap();
        std::os::unix::fs::symlink(high.join("nonexistent"), high.join("broken.desktop")).unwrap();
        std::fs::write(low.join("broken.desktop"), entry).unwrap();
        let mut cache = ApplicationInfoCache::new(vec![high.clone(), low.clone()]);
        assert_eq!(
            cache.get("linked.desktop").path,
            Some(low.join("linked.desktop"))
        );
        assert!(cache.get("broken.desktop").path.is_none());
        assert_eq!(
            cache.get("vendor-example.desktop").path,
            Some(high.join("vendor/example.desktop"))
        );
        assert!(cache.get("hidden.desktop").path.is_none());
        assert!(cache.get("missing.desktop").path.is_none());
        assert!(cache.get("../hidden.desktop").path.is_none());
        let direct = cache.get("handler.desktop");
        assert!(direct.path.is_some());
        let apps = cache.all();
        assert_eq!(apps.len(), 4);
        let mut indexed = ApplicationInfoCache::new(vec![high, low]);
        let indexed_apps = indexed.all();
        for app in apps {
            assert_eq!(app.path, indexed.get(&app.desktop_id).path);
        }
        assert_eq!(indexed_apps.len(), 4);
    }

    #[test]
    fn application_details_are_parsed_once_per_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("example.desktop");
        std::fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=Original\nComment=Details\nExec=example\n",
        )
        .unwrap();
        let mut cache = ApplicationInfoCache::new(vec![directory.path().to_owned()]);

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
        assert_eq!(cache.all()[0].name.as_deref(), Some("Original"));
    }
}
