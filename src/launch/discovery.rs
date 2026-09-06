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
    let mut items = Vec::new();
    let mut desktop_ids = HashSet::new();
    let current_desktops = current_desktops();
    let path_dirs = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    let (path_executables, executable_names) = if include_path {
        let (names, seen) = discover_path_executables(&path_dirs);
        (names, Some(seen))
    } else {
        (Vec::new(), None)
    };

    for data_dir in super::get_xdg_data_dirs() {
        let applications = data_dir.join("applications");
        discover_desktop_directory(
            &applications,
            &applications,
            &mut desktop_ids,
            &current_desktops,
            &path_dirs,
            executable_names.as_ref(),
            &mut items,
        );
    }

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

fn discover_desktop_directory(
    root: &Path,
    directory: &Path,
    seen_ids: &mut HashSet<String>,
    current_desktops: &HashSet<String>,
    path_dirs: &[PathBuf],
    executable_names: Option<&HashSet<String>>,
    items: &mut Vec<LaunchItem>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            discover_desktop_directory(
                root,
                &path,
                seen_ids,
                current_desktops,
                path_dirs,
                executable_names,
                items,
            );
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
        if !seen_ids.insert(id.clone()) {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(entry) = DesktopEntry::parse(&content) else {
            continue;
        };
        if entry.hidden || entry.no_display {
            continue;
        }
        if entry.entry_type != Some("Application") {
            continue;
        }
        if entry.exec.is_none_or(str::is_empty) {
            continue;
        }
        if let Some(try_exec) = entry.try_exec
            && !command_exists(try_exec, path_dirs, executable_names)
        {
            continue;
        }
        if !visible_on_desktops(entry.only_show_in, entry.not_show_in, current_desktops) {
            continue;
        }
        items.push(LaunchItem::DesktopApp {
            id,
            name: entry.name.unwrap_or_default().to_string(),
            path,
        });
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

        let mut seen = HashSet::new();
        let mut items = Vec::new();
        let current = HashSet::new();
        discover_desktop_directory(&high, &high, &mut seen, &current, &[], None, &mut items);
        discover_desktop_directory(&low, &low, &mut seen, &current, &[], None, &mut items);
        assert!(items.is_empty());
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

        let mut seen = HashSet::new();
        let mut items = Vec::new();
        discover_desktop_directory(
            root.path(),
            root.path(),
            &mut seen,
            &HashSet::new(),
            &[],
            None,
            &mut items,
        );
        assert!(matches!(
            &items[0],
            LaunchItem::DesktopApp { id, path: actual, .. }
                if id == "vendor-example.desktop" && actual == &path
        ));
    }
}
