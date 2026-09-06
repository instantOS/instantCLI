//! Shared implementation for the i3-family IPC tree compositors (i3, sway).
//!
//! i3 and sway expose near-identical scratchpad semantics over `i3-msg` /
//! `swaymsg` with the same JSON tree shape. Everything here is parameterized
//! by an [`IpcConfig`] so each backend only contributes its deltas: the IPC
//! binary, the window selector syntax (`[class=...]` vs `[app_id=...]`), the
//! node-id extraction, the hide command, and a post-spawn settle delay.

use super::ScratchpadWindowInfo;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;
use std::time::Duration;

/// Per-compositor deltas for the shared i3-family implementation.
pub(crate) struct IpcConfig {
    /// Human-readable name used in error messages ("i3", "Sway").
    pub name: &'static str,
    /// IPC client binary ("i3-msg", "swaymsg").
    pub bin: &'static str,
    /// Criteria selector for a window id, e.g. `[class="x"]` / `[app_id="x"]`.
    pub selector: fn(&str) -> String,
    /// Extract a node's window id (class for i3, app_id for sway).
    pub id_of: fn(&Value) -> Option<String>,
    /// Tree string keys whose value identifies a window (i3 also matches
    /// `instance`); used by the substring-based `window_exists`.
    pub id_keys: &'static [&'static str],
    /// Suffix appended to the selector to hide a window
    /// ("move scratchpad" vs "move to scratchpad").
    pub hide_suffix: &'static str,
    /// Extra settle delay after the window appears, before configuring it.
    pub post_spawn_settle: Duration,
}

pub(crate) const I3_CONFIG: IpcConfig = IpcConfig {
    name: "i3",
    bin: "i3-msg",
    selector: i3_selector,
    id_of: window_class_of,
    id_keys: &["class", "instance"],
    hide_suffix: " move scratchpad",
    post_spawn_settle: Duration::from_millis(0),
};

pub(crate) const SWAY_CONFIG: IpcConfig = IpcConfig {
    name: "Sway",
    bin: "swaymsg",
    selector: sway_selector,
    id_of: window_app_id_of,
    id_keys: &["app_id"],
    hide_suffix: " move to scratchpad",
    post_spawn_settle: Duration::from_millis(200),
};

fn i3_selector(id: &str) -> String {
    format!("[class=\"{id}\"]")
}

fn sway_selector(id: &str) -> String {
    format!("[app_id=\"{id}\"]")
}

/// Get window class from an i3 tree node (falls back to `instance`).
pub(crate) fn window_class_of(node: &Value) -> Option<String> {
    node.get("window_properties")
        .and_then(|wp| wp.get("class"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            node.get("window_properties")
                .and_then(|wp| wp.get("instance"))
                .and_then(|i| i.as_str())
                .map(|s| s.to_string())
        })
}

/// Get window app_id from a sway tree node.
pub(crate) fn window_app_id_of(node: &Value) -> Option<String> {
    node.get("app_id")
        .and_then(|a| a.as_str())
        .map(|s| s.to_string())
}

/// Execute an IPC command (e.g. `i3-msg '[class="x"] scratchpad show'`).
pub(crate) fn msg(config: &IpcConfig, command: &str) -> Result<String> {
    let output = Command::new(config.bin)
        .args([command])
        .output()
        .context(format!("Failed to execute {}", config.bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} failed: {}", config.bin, stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Execute `<bin> -t get_tree` and return the raw JSON string.
pub(crate) fn get_tree(config: &IpcConfig) -> Result<String> {
    let output = Command::new(config.bin)
        .args(["-t", "get_tree"])
        .output()
        .context(format!("Failed to execute {} -t get_tree", config.bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} -t get_tree failed: {}", config.bin, stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check whether a window id appears in the tree (substring match on the id
/// keys; i3 additionally matches `instance`).
pub(crate) fn window_exists(config: &IpcConfig, window_id: &str) -> Result<bool> {
    let tree = get_tree(config)?;
    Ok(config
        .id_keys
        .iter()
        .any(|key| tree.contains(&format!("\"{key}\": \"{window_id}\""))))
}

/// Check if a window is currently visible (not in the scratchpad).
pub(crate) fn is_window_visible(config: &IpcConfig, window_id: &str) -> Result<bool> {
    let tree = get_tree(config)?;
    let parsed: Value = serde_json::from_str(&tree)
        .context(format!("Failed to parse {} tree JSON", config.name))?;

    find_window_visibility(&parsed, config, window_id)
}

/// Show a scratchpad window (idempotent: no-op when already visible).
pub(crate) fn show_scratchpad(config: &IpcConfig, window_id: &str) -> Result<()> {
    if is_window_visible(config, window_id)? {
        // Window is already visible, do nothing
        return Ok(());
    }

    let message = format!("{} scratchpad show", (config.selector)(window_id));
    msg(config, &message)?;
    Ok(())
}

/// Hide a scratchpad window (idempotent: no-op when already hidden).
pub(crate) fn hide_scratchpad(config: &IpcConfig, window_id: &str) -> Result<()> {
    if !is_window_visible(config, window_id)? {
        // Window is already hidden, do nothing
        return Ok(());
    }

    let message = format!("{}{}", (config.selector)(window_id), config.hide_suffix);
    msg(config, &message)?;
    Ok(())
}

/// Toggle scratchpad window visibility (maintained for compatibility).
pub(crate) fn toggle_scratchpad(config: &IpcConfig, window_id: &str) -> Result<()> {
    let message = format!("{} scratchpad show", (config.selector)(window_id));
    msg(config, &message)?;
    Ok(())
}

/// Configure a window for scratchpad use (floating, sized, centered,
/// moved to the scratchpad).
pub(crate) fn configure_scratchpad_window(
    config: &IpcConfig,
    window_id: &str,
    width_pct: u32,
    height_pct: u32,
) -> Result<()> {
    let selector = (config.selector)(window_id);
    let config_commands = vec![
        format!("{selector} floating enable"),
        format!("{selector} resize set width {width_pct} ppt height {height_pct} ppt"),
        format!("{selector} move position center"),
        format!("{selector}{}", config.hide_suffix),
    ];

    for cmd in config_commands {
        if let Err(e) = msg(config, &cmd) {
            eprintln!("Warning: Failed to configure window: {e}");
        }
    }

    Ok(())
}

/// Get all scratchpad windows from the tree.
pub(crate) fn get_all_scratchpad_windows(config: &IpcConfig) -> Result<Vec<ScratchpadWindowInfo>> {
    let tree = get_tree(config)?;
    let parsed: Value = serde_json::from_str(&tree)
        .context(format!("Failed to parse {} tree JSON", config.name))?;

    let mut scratchpads = Vec::new();

    // Recursively search for scratchpad windows
    if let Some(nodes) = find_scratchpad_nodes(&parsed) {
        for node in nodes {
            if let (Some(name), Some(id)) = (get_window_name(node), (config.id_of)(node)) {
                // Check if this is a scratchpad window (id starts with "scratchpad_")
                if let Some(scratchpad_name) = id.strip_prefix("scratchpad_") {
                    let is_visible = get_node_visible_field(node).unwrap_or(false);
                    scratchpads.push(ScratchpadWindowInfo {
                        name: scratchpad_name.to_string(),
                        window_class: id,
                        title: name,
                        visible: is_visible,
                    });
                }
            }
        }
    }

    Ok(scratchpads)
}

/// Spawn the terminal for a scratchpad window and wait for it to appear,
/// then configure it for scratchpad use.
pub(crate) fn create_and_wait(config: &IpcConfig, scratchpad: &ScratchpadConfig) -> Result<()> {
    let window_id = scratchpad.window_class();
    super::create_terminal_process(scratchpad)?;

    // Wait for window
    let mut attempts = 0;
    while attempts < 30 {
        if window_exists(config, &window_id)? {
            if !config.post_spawn_settle.is_zero() {
                // Give the window a moment to initialize before configuring
                std::thread::sleep(config.post_spawn_settle);
            }
            configure_scratchpad_window(
                config,
                &window_id,
                scratchpad.width_pct,
                scratchpad.height_pct,
            )?;
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        attempts += 1;
    }

    Err(anyhow::anyhow!("Terminal window did not appear"))
}

/// Get the visible field from a node directly
fn get_node_visible_field(node: &Value) -> Option<bool> {
    node.get("visible").and_then(|v| v.as_bool())
}

/// Recursively find all scratchpad nodes in the tree
fn find_scratchpad_nodes(tree: &Value) -> Option<Vec<&Value>> {
    let mut scratchpad_nodes = Vec::new();
    find_nodes_recursive(tree, &mut scratchpad_nodes);
    Some(scratchpad_nodes)
}

/// Recursive helper to find scratchpad nodes
fn find_nodes_recursive<'a>(node: &'a Value, scratchpad_nodes: &mut Vec<&'a Value>) {
    if let Some(nodes) = node.get("nodes").and_then(|n| n.as_array()) {
        for child in nodes {
            // Check if this node has scratchpad state
            if child.get("scratchpad_state").is_some() {
                scratchpad_nodes.push(child);
            }
            // Recursively search children
            find_nodes_recursive(child, scratchpad_nodes);
        }
    }

    // Also check floating nodes
    if let Some(floating_nodes) = node.get("floating_nodes").and_then(|n| n.as_array()) {
        for child in floating_nodes {
            if child.get("scratchpad_state").is_some() {
                scratchpad_nodes.push(child);
            }
            find_nodes_recursive(child, scratchpad_nodes);
        }
    }
}

/// Get window name from node
fn get_window_name(node: &Value) -> Option<String> {
    node.get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

/// Find window visibility by searching the tree
fn find_window_visibility(tree: &Value, config: &IpcConfig, window_id: &str) -> Result<bool> {
    if let Some(visible) = find_window_recursive(tree, config, window_id) {
        Ok(visible)
    } else {
        // Window not found, assume not visible
        Ok(false)
    }
}

/// Recursive helper to find window and check visibility
fn find_window_recursive(node: &Value, config: &IpcConfig, window_id: &str) -> Option<bool> {
    // Check if this node matches our window id
    if let Some(id) = (config.id_of)(node)
        && id == window_id
    {
        // Return the visible field
        return node.get("visible").and_then(|v| v.as_bool());
    }

    // Search in nodes
    if let Some(nodes) = node.get("nodes").and_then(|n| n.as_array()) {
        for child in nodes {
            if let Some(visible) = find_window_recursive(child, config, window_id) {
                return Some(visible);
            }
        }
    }

    // Search in floating nodes
    if let Some(floating_nodes) = node.get("floating_nodes").and_then(|n| n.as_array()) {
        for child in floating_nodes {
            if let Some(visible) = find_window_recursive(child, config, window_id) {
                return Some(visible);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_and_hide_commands_match_per_compositor_syntax() {
        let window_id = "test_class";

        assert_eq!((I3_CONFIG.selector)(window_id), "[class=\"test_class\"]");
        assert_eq!(
            format!(
                "{}{}",
                (I3_CONFIG.selector)(window_id),
                I3_CONFIG.hide_suffix
            ),
            "[class=\"test_class\"] move scratchpad"
        );

        assert_eq!((SWAY_CONFIG.selector)(window_id), "[app_id=\"test_class\"]");
        assert_eq!(
            format!(
                "{}{}",
                (SWAY_CONFIG.selector)(window_id),
                SWAY_CONFIG.hide_suffix
            ),
            "[app_id=\"test_class\"] move to scratchpad"
        );
    }
}
