//! Fresh application discovery for `ins launch`.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::types::LaunchItem;
use crate::common::desktop_entry::DesktopEntry;

/// Discover the current application corpus. XDG directories are visited in
/// precedence order; the first desktop file for an ID masks every later one,
/// including when that first entry is `Hidden=true`.
pub fn discover_launch_items(include_path: bool) -> Vec<LaunchItem> {
    discover_items(include_path, false)
}

/// Include NoDisplay handlers, which are valid Open With choices.
pub(crate) fn discover_file_handlers() -> Vec<LaunchItem> {
    discover_items(false, true)
}

fn discover_items(include_path: bool, handlers: bool) -> Vec<LaunchItem> {
    let current_desktops = current_desktops();
    let path_dirs = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    let (path_executables, executable_names) = if include_path {
        let (names, seen) = discover_path_executables(&path_dirs);
        (names, Some(seen))
    } else {
        (Vec::new(), None)
    };

    let mut scan = DesktopScan::new(
        &current_desktops,
        &path_dirs,
        executable_names.as_ref(),
        handlers,
    );
    for data_dir in crate::applications::data_dirs() {
        let applications = data_dir.join("applications");
        scan.directory(&applications, &applications);
    }
    let mut items = scan.items;

    if !path_executables.is_empty() {
        let desktop_names = items
            .iter()
            .filter_map(|item| match item {
                LaunchItem::DesktopApp { name, .. } => Some(name.to_lowercase()),
                LaunchItem::PathExecutable { .. } => None,
            })
            .collect::<HashSet<_>>();

        for name in path_executables {
            let display_name = if desktop_names.contains(&name.to_lowercase()) {
                format!("path:{name}")
            } else {
                name.clone()
            };
            items.push(LaunchItem::PathExecutable { name, display_name });
        }
    }

    items.sort_by_cached_key(LaunchItem::sort_key);
    items
}

/// Terminal mode lists commands without scanning desktop entries.
pub fn discover_terminal_items() -> Vec<LaunchItem> {
    let paths = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    let (names, _) = discover_path_executables(&paths);
    let mut items = names
        .into_iter()
        .map(|name| LaunchItem::PathExecutable {
            display_name: name.clone(),
            name,
        })
        .collect::<Vec<_>>();
    items.sort_by_cached_key(LaunchItem::sort_key);
    items
}

fn discover_path_executables(path_dirs: &[PathBuf]) -> (Vec<String>, HashSet<String>) {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for directory in path_dirs {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !is_executable_file(&metadata) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    (names, seen)
}

struct DesktopScan<'a> {
    seen_ids: HashSet<String>,
    current_desktops: &'a HashSet<String>,
    path_dirs: &'a [PathBuf],
    executable_names: Option<&'a HashSet<String>>,
    items: Vec<LaunchItem>,
    handlers: bool,
}

impl<'a> DesktopScan<'a> {
    fn new(
        current_desktops: &'a HashSet<String>,
        path_dirs: &'a [PathBuf],
        executable_names: Option<&'a HashSet<String>>,
        handlers: bool,
    ) -> Self {
        Self {
            seen_ids: HashSet::new(),
            current_desktops,
            path_dirs,
            executable_names,
            items: Vec::new(),
            handlers,
        }
    }

    fn directory(&mut self, root: &Path, directory: &Path) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                self.directory(root, &path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let id = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("-");
            if !self.seen_ids.insert(id.clone()) {
                continue;
            }

            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let Some(entry) = DesktopEntry::parse(&content) else {
                continue;
            };
            if entry.hidden || (!self.handlers && entry.no_display) {
                continue;
            }
            if entry.entry_type != Some("Application") {
                continue;
            }
            if entry.exec.is_none_or(str::is_empty) {
                continue;
            }
            if let Some(try_exec) = entry.try_exec
                && !command_exists(try_exec, self.path_dirs, self.executable_names)
            {
                continue;
            }
            if !self.handlers
                && !visible_on_desktops(
                    entry.only_show_in,
                    entry.not_show_in,
                    self.current_desktops,
                )
            {
                continue;
            }
            self.items.push(LaunchItem::DesktopApp {
                id,
                name: entry.name.unwrap_or_default().to_string(),
                path,
            });
        }
    }
}

fn current_desktops() -> HashSet<String> {
    env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|name| !name.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn visible_on_desktops(only: Option<&str>, not: Option<&str>, current: &HashSet<String>) -> bool {
    let matches_current = |name: &str| {
        current
            .iter()
            .any(|desktop| desktop.eq_ignore_ascii_case(name))
    };
    if let Some(names) = not
        && names.split(';').any(matches_current)
    {
        return false;
    }
    match only {
        Some(names) if !names.is_empty() => names.split(';').any(matches_current),
        _ => true,
    }
}

fn command_exists(
    command: &str,
    path_dirs: &[PathBuf],
    executable_names: Option<&HashSet<String>>,
) -> bool {
    let path = PathBuf::from(command);
    if path.is_absolute() {
        return fs::metadata(path).is_ok_and(|metadata| is_executable_file(&metadata));
    }
    if let Some(names) = executable_names {
        return names.contains(command);
    }
    path_dirs.iter().any(|directory| {
        fs::metadata(directory.join(command)).is_ok_and(|metadata| is_executable_file(&metadata))
    })
}

fn is_executable_file(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn desktop_visibility_honors_only_and_not_lists() {
        let current = ["sway".to_string(), "gnome".to_string()]
            .into_iter()
            .collect();
        assert!(visible_on_desktops(Some("GNOME;"), None, &current));
        assert!(!visible_on_desktops(Some("KDE;"), None, &current));
        assert!(!visible_on_desktops(None, Some("Sway;"), &current));
    }

    #[test]
    fn higher_priority_hidden_entry_masks_lower_entry() {
        let root = tempdir().unwrap();
        let high = root.path().join("high");
        let low = root.path().join("low");
        fs::create_dir_all(&high).unwrap();
        fs::create_dir_all(&low).unwrap();
        fs::write(
            high.join("example.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nHidden=true\nExec=hidden\n",
        )
        .unwrap();
        fs::write(
            low.join("example.desktop"),
            "[Desktop Entry]\nType=Application\nName=Visible\nExec=visible\n",
        )
        .unwrap();

        let current = HashSet::new();
        let mut scan = DesktopScan::new(&current, &[], None, true);
        scan.directory(&high, &high);
        scan.directory(&low, &low);
        let items = scan.items;
        assert!(items.is_empty());
    }

    #[test]
    fn file_handlers_include_nodisplay_and_ignore_desktop_visibility() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("handler.desktop"),
            "[Desktop Entry]\nType=Application\nName=Handler\nExec=handler %f\nNoDisplay=true\nOnlyShowIn=OtherDesktop;\n").unwrap();
        let current = HashSet::new();
        let mut launcher = DesktopScan::new(&current, &[], None, false);
        launcher.directory(root.path(), root.path());
        assert!(launcher.items.is_empty());
        let mut handlers = DesktopScan::new(&current, &[], None, true);
        handlers.directory(root.path(), root.path());
        assert_eq!(handlers.items.len(), 1);
    }

    #[test]
    fn nested_desktop_file_gets_spec_id_and_path() {
        let root = tempdir().unwrap();
        let nested = root.path().join("vendor");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("example.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=Example\nExec=example\n",
        )
        .unwrap();

        let current = HashSet::new();
        let mut scan = DesktopScan::new(&current, &[], None, false);
        scan.directory(root.path(), root.path());
        let items = scan.items;
        assert!(matches!(
            &items[0],
            LaunchItem::DesktopApp { id, path: actual, .. }
                if id == "vendor-example.desktop" && actual == &path
        ));
    }
}
