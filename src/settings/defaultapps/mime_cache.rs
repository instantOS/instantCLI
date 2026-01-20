use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap};
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
    let apps: BTreeSet<String> = mime_map
        .get(mime_type)
        .map(|apps| apps.iter().cloned().collect())
        .unwrap_or_default();

    apps.into_iter().collect()
}

fn get_mimeinfo_cache_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        paths.push(home_path.join(".local/share/applications/mimeinfo.cache"));
        paths
            .push(home_path.join(".local/share/flatpak/exports/share/applications/mimeinfo.cache"));
    }

    paths.push(PathBuf::from(
        "/var/lib/flatpak/exports/share/applications/mimeinfo.cache",
    ));
    paths.push(PathBuf::from("/usr/share/applications/mimeinfo.cache"));

    if let Ok(xdg_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in xdg_dirs.split(':') {
            if !dir.is_empty() {
                paths.push(PathBuf::from(dir).join("applications/mimeinfo.cache"));
            }
        }
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
