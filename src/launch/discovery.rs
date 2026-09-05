//! Fresh application discovery for `ins launch`.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use freedesktop_file_parser::{EntryType, parse};

use super::types::LaunchItem;

/// Discover the current application corpus. XDG directories are visited in
/// precedence order; the first desktop file for an ID masks every later one,
/// including when that first entry is `Hidden=true`.
pub fn discover_launch_items() -> Vec<LaunchItem> {
    let mut items = Vec::new();
    let mut desktop_ids = HashSet::new();

    for data_dir in super::get_xdg_data_dirs() {
        let applications = data_dir.join("applications");
        discover_desktop_directory(&applications, &applications, &mut desktop_ids, &mut items);
    }

    let desktop_names = items
        .iter()
        .filter_map(|item| match item {
            LaunchItem::DesktopApp { name, .. } => Some(name.to_lowercase()),
            LaunchItem::PathExecutable { .. } => None,
        })
        .collect::<HashSet<_>>();

    let mut executable_names = HashSet::new();
    for directory in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
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
            if executable_names.insert(name.clone()) {
                let display_name = if desktop_names.contains(&name.to_lowercase()) {
                    format!("path:{name}")
                } else {
                    name.clone()
                };
                items.push(LaunchItem::PathExecutable { name, display_name });
            }
        }
    }

    items.sort_by_key(LaunchItem::sort_key);
    items
}

fn discover_desktop_directory(
    root: &Path,
    directory: &Path,
    seen_ids: &mut HashSet<String>,
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
            discover_desktop_directory(root, &path, seen_ids, items);
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
        let Ok(file) = parse(&content) else {
            continue;
        };
        if file.entry.hidden.unwrap_or(false) || file.entry.no_display.unwrap_or(false) {
            continue;
        }
        let EntryType::Application(application) = file.entry.entry_type else {
            continue;
        };
        if application.exec.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        if let Some(try_exec) = application.try_exec.as_deref()
            && !command_exists(try_exec)
        {
            continue;
        }
        if !visible_on_current_desktop(
            file.entry.only_show_in.as_deref(),
            file.entry.not_show_in.as_deref(),
        ) {
            continue;
        }
        items.push(LaunchItem::DesktopApp {
            id,
            name: file.entry.name.default,
            path,
        });
    }
}

fn visible_on_current_desktop(only: Option<&[String]>, not: Option<&[String]>) -> bool {
    let current = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|name| !name.is_empty())
        .map(str::to_lowercase)
        .collect::<HashSet<_>>();
    visible_on_desktops(only, not, &current)
}

fn visible_on_desktops(
    only: Option<&[String]>,
    not: Option<&[String]>,
    current: &HashSet<String>,
) -> bool {
    let matches_current = |name: &str| {
        current
            .iter()
            .any(|desktop| desktop.eq_ignore_ascii_case(name))
    };
    if let Some(names) = not
        && names.iter().any(|name| matches_current(name))
    {
        return false;
    }
    match only {
        Some(names) if !names.is_empty() => names.iter().any(|name| matches_current(name)),
        _ => true,
    }
}

fn command_exists(command: &str) -> bool {
    let path = PathBuf::from(command);
    if path.is_absolute() {
        return fs::metadata(path).is_ok_and(|metadata| is_executable_file(&metadata));
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default()).any(|directory| {
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
        assert!(visible_on_desktops(Some(&["GNOME".into()]), None, &current));
        assert!(!visible_on_desktops(Some(&["KDE".into()]), None, &current));
        assert!(!visible_on_desktops(None, Some(&["Sway".into()]), &current));
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
        discover_desktop_directory(&high, &high, &mut seen, &mut items);
        discover_desktop_directory(&low, &low, &mut seen, &mut items);
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
        discover_desktop_directory(root.path(), root.path(), &mut seen, &mut items);
        assert!(matches!(
            &items[0],
            LaunchItem::DesktopApp { id, path: actual, .. }
                if id == "vendor-example.desktop" && actual == &path
        ));
    }
}
