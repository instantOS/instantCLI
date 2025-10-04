use anyhow::{Context, Result};
use freedesktop_file_parser::{parse, EntryType};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;

use super::context::SettingsContext;

/// Get all XDG data directories where desktop files may be located
fn get_desktop_file_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // User local applications (highest priority)
    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        dirs.push(home_path.join(".local/share/applications"));
        
        // User flatpak apps
        dirs.push(home_path.join(".local/share/flatpak/exports/share/applications"));
    }

    // System flatpak apps
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));

    // System applications directory
    dirs.push(PathBuf::from("/usr/share/applications"));

    // Additional XDG data dirs from environment
    if let Ok(xdg_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in xdg_dirs.split(':') {
            if !dir.is_empty() {
                dirs.push(PathBuf::from(dir).join("applications"));
            }
        }
    }

    // Filter to only existing directories
    dirs.into_iter().filter(|d| d.exists()).collect()
}

/// Parse a desktop file and extract its supported MIME types
fn parse_desktop_file_mime_types(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read desktop file: {}", path.display()))?;
    
    let desktop_file = parse(&content)
        .with_context(|| format!("Failed to parse desktop file: {}", path.display()))?;

    // Only look at Application entries
    if let EntryType::Application(app) = &desktop_file.entry.entry_type {
        if let Some(mime_types) = &app.mime_type {
            return Ok(mime_types.clone());
        }
    }

    Ok(Vec::new())
}

/// Build a mapping of MIME types to desktop files by scanning all desktop files
fn build_mime_to_apps_map() -> Result<HashMap<String, Vec<String>>> {
    let mut mime_map: HashMap<String, Vec<String>> = HashMap::new();
    let directories = get_desktop_file_directories();

    for dir in directories {
        // Walk through the directory looking for .desktop files
        for entry in WalkDir::new(&dir)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            
            // Only process .desktop files
            if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }

            // Get the desktop file ID (filename)
            let desktop_id = match path.file_name().and_then(|s| s.to_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            // Parse the desktop file and get its MIME types
            match parse_desktop_file_mime_types(path) {
                Ok(mime_types) => {
                    for mime_type in mime_types {
                        mime_map
                            .entry(mime_type)
                            .or_default()
                            .push(desktop_id.clone());
                    }
                }
                Err(err) => {
                    // Skip files we can't parse (they might be broken or have syntax errors)
                    emit(
                        Level::Debug,
                        "settings.defaultapps.parse_failed",
                        &format!(
                            "{} Failed to parse {}: {err}",
                            char::from(NerdFont::Warning),
                            path.display()
                        ),
                        None,
                    );
                }
            }
        }
    }

    Ok(mime_map)
}

/// Get all available MIME types by scanning desktop files
fn get_all_mime_types() -> Result<Vec<String>> {
    let mime_map = build_mime_to_apps_map()?;
    let mime_types: BTreeSet<String> = mime_map.keys().cloned().collect();
    Ok(mime_types.into_iter().collect())
}

/// Get applications for a specific MIME type
fn get_apps_for_mime(mime_type: &str) -> Result<Vec<String>> {
    let mime_map = build_mime_to_apps_map()?;
    
    // Use BTreeSet to remove duplicates and sort
    let apps: BTreeSet<String> = mime_map
        .get(mime_type)
        .map(|apps| apps.iter().cloned().collect())
        .unwrap_or_default();
    
    Ok(apps.into_iter().collect())
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

    // Get all MIME types
    let mime_types = get_all_mime_types().context("Failed to get MIME types")?;

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
    let apps = get_apps_for_mime(&selected_mime).context("Failed to get applications")?;

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
