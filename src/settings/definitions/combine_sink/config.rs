use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{
    COMBINE_SINK_CONFIG_FILE, DEFAULT_COMBINED_SINK_NAME, INS_COMBINED_SINK_PREFIX,
    PIPEWIRE_CONFIG_DIR,
};

/// Get the path to the PipeWire config directory
pub(super) fn pipewire_config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("Unable to determine user config directory")?;
    Ok(config_dir.join(PIPEWIRE_CONFIG_DIR))
}

/// Get the full path to the combine-sink config file
pub(super) fn combine_sink_config_file() -> Result<PathBuf> {
    Ok(pipewire_config_path()?.join(COMBINE_SINK_CONFIG_FILE))
}

/// Check if the combined sink is currently enabled (config file exists)
pub(super) fn is_combined_sink_enabled() -> bool {
    combine_sink_config_file()
        .map(|path| path.exists())
        .unwrap_or(false)
}

/// Generate a sanitized node name from a display name
/// Converts "My Combined Output" -> "ins_combined_my_combined_output"
pub(super) fn display_name_to_node_name(display_name: &str) -> String {
    let sanitized = display_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    // Collapse multiple underscores
    let collapsed = sanitized
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    format!("{}{}", INS_COMBINED_SINK_PREFIX, collapsed)
}

/// Parse config file to extract device node names and display name
fn parse_config_file() -> Result<Option<(Vec<String>, String)>> {
    let content = match read_current_config()? {
        Some(c) => c,
        None => return Ok(None),
    };

    // Extract device node names from `node.name = "..."` inside stream.rules matches
    let mut devices = Vec::new();
    let mut in_matches = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("matches = [") {
            in_matches = true;
            continue;
        }
        if in_matches && trimmed.starts_with(']') {
            in_matches = false;
            continue;
        }
        if in_matches {
            // Look for: { node.name = "device_name" }
            if let Some(start) = trimmed.find("node.name = \"") {
                let after = &trimmed[start + 13..]; // after `node.name = "`
                if let Some(end) = after.find('"') {
                    devices.push(after[..end].to_string());
                }
            }
        }
    }

    // Extract display name from node.description
    let display_name = content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if let Some(start) = trimmed.strip_prefix("node.description = \"") {
                start.strip_suffix('"').map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| DEFAULT_COMBINED_SINK_NAME.to_string());

    if devices.is_empty() {
        Ok(None)
    } else {
        Ok(Some((devices, display_name)))
    }
}

/// Get the current combined sink configuration from the config file
pub(super) fn get_current_config() -> (HashSet<String>, String) {
    match parse_config_file() {
        Ok(Some((devices, name))) => (devices.into_iter().collect(), name),
        _ => (HashSet::new(), DEFAULT_COMBINED_SINK_NAME.to_string()),
    }
}

/// Get the current combined sink name from the config file, or the default if none is set
pub(super) fn get_current_sink_name() -> String {
    get_current_config().1
}

/// Read the current config file content if it exists
fn read_current_config() -> Result<Option<String>> {
    let config_path = combine_sink_config_file()?;
    if !config_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {:?}", config_path))?;
    Ok(Some(content))
}

/// Check if the desired config differs from current config
/// Returns true if config changed (restart needed), false if already in desired state
pub(super) fn config_changed(desired_devices: &[String], desired_name: &str) -> Result<bool> {
    let (current_devices, current_name) = get_current_config();

    // If no config exists, we need to create it
    if current_devices.is_empty() && !is_combined_sink_enabled() {
        return Ok(true);
    }

    // Check if name changed
    if current_name != desired_name {
        return Ok(true);
    }

    // Check if device list changed (compare sets, order doesn't matter)
    let desired_set: HashSet<&String> = desired_devices.iter().collect();
    let current_set: HashSet<&String> = current_devices.iter().collect();

    Ok(desired_set != current_set)
}
