use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub(crate) fn build_mime_to_apps_map() -> Result<HashMap<String, Vec<String>>> {
    let mut mime_map: HashMap<String, Vec<String>> = HashMap::new();
    let cache_paths = get_mimeinfo_cache_paths();

    for cache_path in cache_paths {
        match parse_mimeinfo_cache(&cache_path) {
            Ok(cache) => {
                for (mime_type, apps) in cache {
                    mime_map.entry(mime_type).or_default().extend(apps);
                }
            }
            Err(_) => {
                continue;
            }
        }
    }

    Ok(mime_map)
}

pub(crate) fn get_apps_for_mime(
    mime_type: &str,
    mime_map: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut lookup_types: BTreeSet<String> = BTreeSet::new();
    lookup_types.insert(mime_type.to_string());

    let canonical = canonical_mime_type(mime_type);
    lookup_types.insert(canonical.clone());

    for parent in mime_parent_types(&canonical) {
        lookup_types.insert(parent);
    }

    let mut apps: BTreeSet<String> = BTreeSet::new();
    for lookup in lookup_types {
        if let Some(entries) = mime_map.get(&lookup) {
            apps.extend(entries.iter().cloned());
        }
    }

    apps.into_iter().collect()
}

fn get_mimeinfo_cache_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(data_home) = xdg_data_home() {
        paths.push(data_home.join("applications/mimeinfo.cache"));
        paths.push(data_home.join("flatpak/exports/share/applications/mimeinfo.cache"));
    }

    paths.push(PathBuf::from(
        "/var/lib/flatpak/exports/share/applications/mimeinfo.cache",
    ));

    for dir in xdg_data_dirs() {
        paths.push(dir.join("applications/mimeinfo.cache"));
    }

    paths.into_iter().filter(|p| p.exists()).collect()
}

fn parse_mimeinfo_cache(path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open mimeinfo.cache at {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    let mut in_mime_cache = false;
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line == "[MIME Cache]" {
            in_mime_cache = true;
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_mime_cache = false;
            continue;
        }

        if in_mime_cache && let Some((mime_type, apps)) = line.split_once('=') {
            let apps: Vec<String> = apps
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            if !apps.is_empty() {
                map.entry(mime_type.to_string()).or_default().extend(apps);
            }
        }
    }

    Ok(map)
}

fn canonical_mime_type(mime_type: &str) -> String {
    for path in mime_alias_paths() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let alias = parts.next().unwrap_or("");
                let canonical = parts.next().unwrap_or("");
                if alias == mime_type && !canonical.is_empty() {
                    return canonical.to_string();
                }
            }
        }
    }

    mime_type.to_string()
}

fn mime_parent_types(mime_type: &str) -> Vec<String> {
    let subclass_map = load_mime_subclasses();
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    let mut parents = Vec::new();

    queue.push_back(mime_type.to_string());

    while let Some(current) = queue.pop_front() {
        if let Some(entries) = subclass_map.get(&current) {
            for parent in entries {
                if seen.insert(parent.clone()) {
                    parents.push(parent.clone());
                    queue.push_back(parent.clone());
                }
            }
        }
    }

    parents
}

fn load_mime_subclasses() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    for path in mime_subclass_paths() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let child = parts.next().unwrap_or("");
                let parent = parts.next().unwrap_or("");
                if child.is_empty() || parent.is_empty() {
                    continue;
                }
                map.entry(child.to_string())
                    .or_default()
                    .push(parent.to_string());
            }
        }
    }

    map
}

fn mime_alias_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(data_home) = xdg_data_home() {
        paths.push(data_home.join("mime/aliases"));
    }

    for dir in xdg_data_dirs() {
        paths.push(dir.join("mime/aliases"));
    }

    paths.into_iter().filter(|p| p.exists()).collect()
}

fn mime_subclass_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(data_home) = xdg_data_home() {
        paths.push(data_home.join("mime/subclasses"));
    }

    for dir in xdg_data_dirs() {
        paths.push(dir.join("mime/subclasses"));
    }

    paths.into_iter().filter(|p| p.exists()).collect()
}

fn xdg_data_home() -> Option<PathBuf> {
    if let Some(path) = env::var_os("XDG_DATA_HOME")
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }

    env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
}

fn xdg_data_dirs() -> Vec<PathBuf> {
    if let Ok(dirs) = env::var("XDG_DATA_DIRS") {
        let parsed: Vec<PathBuf> = dirs
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
    }

    vec![
        PathBuf::from("/usr/local/share"),
        PathBuf::from("/usr/share"),
    ]
}
