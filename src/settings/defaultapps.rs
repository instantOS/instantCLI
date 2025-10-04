use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;

use super::context::SettingsContext;

/// Get all XDG data directories where mimeinfo.cache files may be located
fn get_mimeinfo_cache_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // User local applications (highest priority)
    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        paths.push(home_path.join(".local/share/applications/mimeinfo.cache"));
        
        // User flatpak apps
        paths.push(home_path.join(".local/share/flatpak/exports/share/applications/mimeinfo.cache"));
    }

    // System flatpak apps
    paths.push(PathBuf::from("/var/lib/flatpak/exports/share/applications/mimeinfo.cache"));

    // System applications directory
    paths.push(PathBuf::from("/usr/share/applications/mimeinfo.cache"));

    // Additional XDG data dirs from environment
    if let Ok(xdg_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in xdg_dirs.split(':') {
            if !dir.is_empty() {
                paths.push(PathBuf::from(dir).join("applications/mimeinfo.cache"));
            }
        }
    }

    // Filter to only existing files
    paths.into_iter().filter(|p| p.exists()).collect()
}

/// Parse a mimeinfo.cache file and return a mapping of MIME types to desktop files
fn parse_mimeinfo_cache(path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open mimeinfo.cache at {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    let mut in_mime_cache = false;
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Check for [MIME Cache] section header
        if line == "[MIME Cache]" {
            in_mime_cache = true;
            continue;
        }

        // Check for other section headers
        if line.starts_with('[') && line.ends_with(']') {
            in_mime_cache = false;
            continue;
        }

        // Parse entries only if we're in the [MIME Cache] section
        if in_mime_cache {
            if let Some((mime_type, apps)) = line.split_once('=') {
                let apps: Vec<String> = apps
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();

                if !apps.is_empty() {
                    map.entry(mime_type.to_string())
                        .or_default()
                        .extend(apps);
                }
            }
        }
    }

    Ok(map)
}

/// Build a mapping of MIME types to desktop files by reading mimeinfo.cache files
/// This is the fast approach that Thunar uses
fn build_mime_to_apps_map() -> Result<HashMap<String, Vec<String>>> {
    let mut mime_map: HashMap<String, Vec<String>> = HashMap::new();
    let cache_paths = get_mimeinfo_cache_paths();

    for cache_path in cache_paths {
        match parse_mimeinfo_cache(&cache_path) {
            Ok(cache) => {
                // Merge this cache into our map
                for (mime_type, apps) in cache {
                    mime_map
                        .entry(mime_type)
                        .or_default()
                        .extend(apps);
                }
            }
            Err(_) => {
                // Silently skip cache files we can't read
                continue;
            }
        }
    }

    Ok(mime_map)
}

/// Get all available MIME types from mimeinfo.cache files
fn get_all_mime_types(mime_map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mime_types: BTreeSet<String> = mime_map.keys().cloned().collect();
    mime_types.into_iter().collect()
}

/// Get applications for a specific MIME type
fn get_apps_for_mime(mime_type: &str, mime_map: &HashMap<String, Vec<String>>) -> Vec<String> {
    // Use BTreeSet to remove duplicates and sort
    let apps: BTreeSet<String> = mime_map
        .get(mime_type)
        .map(|apps| apps.iter().cloned().collect())
        .unwrap_or_default();
    
    apps.into_iter().collect()
}

/// Query the current default application for a MIME type using xdg-mime
fn query_default_app(mime_type: &str) -> Result<Option<String>> {
    let output = Command::new("xdg-mime")
        .arg("query")
        .arg("default")
        .arg(mime_type)
        .output()
        .context("Failed to execute xdg-mime query")?;

    if !output.status.success() {
        return Ok(None);
    }

    let default_app = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if default_app.is_empty() {
        Ok(None)
    } else {
        Ok(Some(default_app))
    }
}

/// Set the default application for a MIME type using xdg-mime
fn set_default_app(mime_type: &str, desktop_file: &str) -> Result<()> {
    let status = Command::new("xdg-mime")
        .arg("default")
        .arg(desktop_file)
        .arg(mime_type)
        .status()
        .context("Failed to execute xdg-mime default")?;

    if !status.success() {
        anyhow::bail!("xdg-mime default command failed");
    }

    Ok(())
}

/// Get a human-readable name for a desktop file
fn get_app_name(desktop_file: &str) -> String {
    // Try to read the desktop file and get the Name field
    let desktop_paths = [
        format!("/usr/share/applications/{}", desktop_file),
        format!(
            "{}/.local/share/applications/{}",
            std::env::var("HOME").unwrap_or_default(),
            desktop_file
        ),
    ];

    for path in &desktop_paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(name) = line.strip_prefix("Name=") {
                    return format!("{} ({})", name.trim(), desktop_file);
                }
            }
        }
    }

    // Fallback to just the desktop file name
    desktop_file.to_string()
}

/// Main action for managing default applications
pub fn manage_default_apps(ctx: &mut SettingsContext) -> Result<()> {
    use crate::menu_utils::FzfResult;

    // Check if xdg-mime is available
    if Command::new("which")
        .arg("xdg-mime")
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(()) } else { None })
        .is_none()
    {
        emit(
            Level::Error,
            "settings.defaultapps.no_xdg_mime",
            &format!(
                "{} xdg-mime command not found. Please install xdg-utils.",
                char::from(NerdFont::CrossCircle)
            ),
            None,
        );
        return Ok(());
    }

    // Build the MIME map once and reuse it
    let mime_map = build_mime_to_apps_map().context("Failed to build MIME type map")?;

    // Get all MIME types
    let mime_types = get_all_mime_types(&mime_map);

    if mime_types.is_empty() {
        emit(
            Level::Warn,
            "settings.defaultapps.no_mime_types",
            &format!(
                "{} No MIME types found in mimeinfo.cache files.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(());
    }

    // Create preview command that shows current default
    let preview_cmd = "echo 'Current default:'; xdg-mime query default {}";

    // Let user select a MIME type
    let selected_mime = match FzfWrapper::builder()
        .prompt("Select MIME type: ")
        .args(["--preview", preview_cmd])
        .select(mime_types)?
    {
        FzfResult::Selected(mime) => mime,
        _ => {
            ctx.emit_info("settings.defaultapps.cancelled", "No MIME type selected.");
            return Ok(());
        }
    };

    // Get applications for this MIME type
    let apps = get_apps_for_mime(&selected_mime, &mime_map);

    if apps.is_empty() {
        emit(
            Level::Warn,
            "settings.defaultapps.no_apps",
            &format!(
                "{} No applications found for {}",
                char::from(NerdFont::Warning),
                selected_mime
            ),
            None,
        );
        return Ok(());
    }

    // Show current default in header
    let current_default = query_default_app(&selected_mime)?;
    let header = if let Some(ref default) = current_default {
        format!("MIME type: {}\nCurrent default: {}", selected_mime, default)
    } else {
        format!("MIME type: {}\nCurrent default: (none)", selected_mime)
    };

    // Format apps with names
    let app_choices: Vec<String> = apps.iter().map(|app| get_app_name(app)).collect();

    // Let user select an application
    let selected_app_display = match FzfWrapper::builder()
        .prompt("Select application: ")
        .header(&header)
        .select(app_choices.clone())?
    {
        FzfResult::Selected(app) => app,
        _ => {
            ctx.emit_info("settings.defaultapps.cancelled", "No application selected.");
            return Ok(());
        }
    };

    // Find the original desktop file name
    let app_index = app_choices
        .iter()
        .position(|a| a == &selected_app_display)
        .context("Failed to find selected app")?;
    let desktop_file = &apps[app_index];

    // Set the default application
    set_default_app(&selected_mime, desktop_file)
        .context("Failed to set default application")?;

    ctx.notify(
        "Default application",
        &format!("Set {} as default for {}", desktop_file, selected_mime),
    );

    Ok(())
}
