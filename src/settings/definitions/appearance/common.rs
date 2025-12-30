//! Common utilities for appearance settings
//!
//! Shared helper functions for GTK theming, gsettings integration, and color picking.

use anyhow::{Context, Result};
use std::process::Command;

// ============================================================================
// GTK Theme Helpers
// ============================================================================

/// List all available GTK themes on the system
pub(crate) fn list_gtk_themes() -> Result<Vec<String>> {
    let mut themes = std::collections::HashSet::new();
    let dirs = [
        dirs::home_dir().map(|p| p.join(".themes")),
        dirs::home_dir().map(|p| p.join(".local/share/themes")),
        Some(std::path::PathBuf::from("/usr/share/themes")),
    ];

    for dir in dirs.into_iter().flatten() {
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                let path = entry.path();
                // Check for index.theme OR gtk-3.0/gtk.css OR gtk-4.0/gtk.css
                if (path.join("index.theme").exists()
                    || path.join("gtk-3.0/gtk.css").exists()
                    || path.join("gtk-4.0/gtk.css").exists())
                    && let Some(name) = entry.file_name().to_str()
                {
                    themes.insert(name.to_string());
                }
            }
        }
    }

    let mut result: Vec<String> = themes.into_iter().collect();
    result.sort();
    Ok(result)
}

/// Check if a theme with the given name exists
pub(crate) fn theme_exists(theme_name: &str) -> bool {
    let dirs = [
        dirs::home_dir().map(|p| p.join(".themes")),
        dirs::home_dir().map(|p| p.join(".local/share/themes")),
        Some(std::path::PathBuf::from("/usr/share/themes")),
    ];

    for dir in dirs.into_iter().flatten() {
        let theme_path = dir.join(theme_name);
        if theme_path.exists()
            && (theme_path.join("index.theme").exists()
                || theme_path.join("gtk-3.0/gtk.css").exists()
                || theme_path.join("gtk-4.0/gtk.css").exists())
        {
            return true;
        }
    }
    false
}

/// Get the current GTK theme name
pub(crate) fn get_current_gtk_theme() -> Result<String> {
    let output = Command::new("timeout")
        .args([
            "2s",
            "gsettings",
            "get",
            "org.gnome.desktop.interface",
            "gtk-theme",
        ])
        .output()
        .context("Failed to query current GTK theme")?;

    let theme = String::from_utf8_lossy(&output.stdout);
    // Remove quotes and whitespace
    Ok(theme
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string())
}

/// Set the GTK theme
pub(crate) fn set_gtk_theme(theme_name: &str) -> Result<()> {
    let status = Command::new("timeout")
        .args([
            "10s",
            "gsettings",
            "set",
            "org.gnome.desktop.interface",
            "gtk-theme",
            theme_name,
        ])
        .status()
        .context("Failed to set GTK theme")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Failed to set GTK theme to {}", theme_name);
    }
}

// ============================================================================
// GTK Icon Theme Helpers
// ============================================================================

/// List all available icon themes on the system
pub(crate) fn list_icon_themes() -> Result<Vec<String>> {
    let mut themes = std::collections::HashSet::new();
    let dirs = [
        dirs::home_dir().map(|p| p.join(".icons")),
        dirs::data_local_dir().map(|p| p.join("icons")),
        Some(std::path::PathBuf::from("/usr/share/icons")),
    ];

    for dir in dirs.into_iter().flatten() {
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                // Check for index.theme
                if entry.path().join("index.theme").exists()
                    && let Some(name) = entry.file_name().to_str()
                {
                    themes.insert(name.to_string());
                }
            }
        }
    }

    let mut result: Vec<String> = themes.into_iter().collect();
    result.sort();
    Ok(result)
}

/// Check if an icon theme with the given name exists
pub(crate) fn icon_theme_exists(theme_name: &str) -> bool {
    let dirs = [
        dirs::home_dir().map(|p| p.join(".icons")),
        dirs::home_dir().map(|p| p.join(".local/share/icons")),
        Some(std::path::PathBuf::from("/usr/share/icons")),
    ];

    for dir in dirs.into_iter().flatten() {
        let theme_path = dir.join(theme_name);
        if theme_path.exists() && theme_path.join("index.theme").exists() {
            return true;
        }
    }
    false
}

/// Get the current icon theme name
pub(crate) fn get_current_icon_theme() -> Result<String> {
    let output = Command::new("timeout")
        .args([
            "2s",
            "gsettings",
            "get",
            "org.gnome.desktop.interface",
            "icon-theme",
        ])
        .output()
        .context("Failed to query current icon theme")?;

    let theme = String::from_utf8_lossy(&output.stdout);
    // Remove quotes and whitespace
    Ok(theme
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string())
}

/// Set the icon theme
pub(crate) fn set_icon_theme(theme_name: &str) -> Result<()> {
    let status = Command::new("timeout")
        .args([
            "10s",
            "gsettings",
            "set",
            "org.gnome.desktop.interface",
            "icon-theme",
            theme_name,
        ])
        .status()
        .context("Failed to set icon theme")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Failed to set icon theme to {}", theme_name);
    }
}

// ============================================================================
// GTK Configuration Helpers
// ============================================================================

/// Update GTK settings.ini file for a specific version
pub(crate) fn update_gtk_config(version: &str, key: &str, value: &str) -> Result<()> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join(format!("gtk-{}", version));

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    let settings_path = config_dir.join("settings.ini");
    let content = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)?
    } else {
        String::new()
    };

    let mut new_lines = Vec::new();
    let mut found_section = false;
    let mut found_key = false;
    let mut in_settings_section = false;

    // Simple parser to update INI file
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[Settings]" {
            found_section = true;
            in_settings_section = true;
            new_lines.push(line.to_string());
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_settings_section = false;
        }

        if in_settings_section && trimmed.starts_with(key) {
            new_lines.push(format!("{}={}", key, value));
            found_key = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found_section {
        if !new_lines.is_empty() && !new_lines.last().unwrap().is_empty() {
            new_lines.push("".to_string());
        }
        new_lines.push("[Settings]".to_string());
        new_lines.push(format!("{}={}", key, value));
    } else if !found_key {
        // Find where to insert the key in the [Settings] section
        // We'll just append it after the [Settings] line for simplicity in this case
        // But since we are rebuilding the list, we need to be careful.
        // Let's re-scan new_lines to find [Settings] and insert after it.
        let mut final_lines = Vec::new();
        for line in new_lines {
            final_lines.push(line.clone());
            if line.trim() == "[Settings]" && !found_key {
                final_lines.push(format!("{}={}", key, value));
                found_key = true;
            }
        }
        new_lines = final_lines;
    }

    let new_content = new_lines.join("\n");
    // Ensure trailing newline
    let final_content = if new_content.ends_with('\n') {
        new_content
    } else {
        format!("{}\n", new_content)
    };

    std::fs::write(settings_path, final_content)?;
    Ok(())
}

/// Apply GTK4 libadwaita overrides by symlinking theme's gtk-4.0 directory
pub(crate) fn apply_gtk4_overrides(theme_name: &str) -> Result<()> {
    // Find the theme directory
    let dirs = [
        dirs::home_dir().map(|p| p.join(".themes")),
        dirs::home_dir().map(|p| p.join(".local/share/themes")),
        Some(std::path::PathBuf::from("/usr/share/themes")),
    ];

    let mut theme_path = None;
    for dir in dirs.into_iter().flatten() {
        let p = dir.join(theme_name);
        if p.exists() {
            theme_path = Some(p);
            break;
        }
    }

    let theme_path = theme_path.context("Theme not found")?;
    let source_gtk4 = theme_path.join("gtk-4.0");

    if !source_gtk4.exists() {
        // Theme doesn't have explicit GTK 4 support - clear any existing overrides
        // so that GTK 4 apps fall back to their default appearance
        let config_dir = dirs::config_dir().context("No config dir")?.join("gtk-4.0");
        if config_dir.exists() {
            let items = ["gtk.css", "gtk-dark.css", "assets"];
            for item in items {
                let target = config_dir.join(item);
                if target.is_symlink() || target.exists() {
                    if target.is_dir() && !target.is_symlink() {
                        let _ = std::fs::remove_dir_all(&target);
                    } else {
                        let _ = std::fs::remove_file(&target);
                    }
                }
            }
        }
        return Err(anyhow::anyhow!("Theme has no gtk-4.0 directory"));
    }

    // Target directory: ~/.config/gtk-4.0/
    let config_dir = dirs::config_dir().context("No config dir")?.join("gtk-4.0");

    // Handle broken symlinks: is_symlink() returns true even for broken symlinks,
    // but exists() returns false. Remove broken symlinks before creating directory.
    if config_dir.is_symlink() && !config_dir.exists() {
        std::fs::remove_file(&config_dir)?;
    }

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    // Items to symlink
    let items = ["gtk.css", "gtk-dark.css", "assets"];

    for item in items {
        let source = source_gtk4.join(item);
        let target = config_dir.join(item);

        // Remove existing target (file or symlink)
        if target.is_symlink() || target.exists() {
            // Use fs::remove_file for files and symlinks (even if they point to dirs)
            // Use fs::remove_dir_all if it's a real directory (not symlink)
            if target.is_dir() && !target.is_symlink() {
                std::fs::remove_dir_all(&target)?;
            } else {
                std::fs::remove_file(&target)?;
            }
        }

        if source.exists() {
            std::os::unix::fs::symlink(&source, &target)
                .with_context(|| format!("Failed to link {:?} -> {:?}", source, target))?;
        }
    }

    Ok(())
}

// ============================================================================
// Cursor Theme Helpers
// ============================================================================

/// List all available cursor themes on the system
pub(crate) fn list_cursor_themes() -> Result<Vec<String>> {
    let mut themes = std::collections::HashSet::new();
    let dirs = [
        dirs::home_dir().map(|p| p.join(".icons")),
        dirs::data_local_dir().map(|p| p.join("icons")),
        Some(std::path::PathBuf::from("/usr/share/icons")),
    ];

    for dir in dirs.into_iter().flatten() {
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                let path = entry.path();
                if let Some(name) = entry.file_name().to_str()
                    && is_cursor_theme(path)
                {
                    themes.insert(name.to_string());
                }
            }
        }
    }

    let mut result: Vec<String> = themes.into_iter().collect();
    result.sort();
    Ok(result)
}

/// Check if a directory is a cursor theme
pub(crate) fn is_cursor_theme(path: &std::path::Path) -> bool {
    path.join("cursors").exists()
}

// ============================================================================
// Color Picking Helpers
// ============================================================================

/// Pick a color using zenity color picker
pub(crate) fn pick_color_with_zenity(title: &str, initial: &str) -> Result<Option<String>> {
    let output = Command::new("zenity")
        .args(["--color-selection", "--title", title, "--color", initial])
        .output()
        .context("Failed to run zenity")?;

    if !output.status.success() {
        return Ok(None);
    }

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if let Some(hex) = rgb_to_hex(&result) {
        Ok(Some(hex))
    } else if result.starts_with('#') {
        Ok(Some(result))
    } else {
        Ok(None)
    }
}

/// Convert RGB string to hex color
pub(crate) fn rgb_to_hex(rgb: &str) -> Option<String> {
    let re = regex::Regex::new(r"rgb\((\d+),(\d+),(\d+)\)").ok()?;
    let caps = re.captures(rgb)?;
    let r: u8 = caps.get(1)?.as_str().parse().ok()?;
    let g: u8 = caps.get(2)?.as_str().parse().ok()?;
    let b: u8 = caps.get(3)?.as_str().parse().ok()?;
    Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
}
