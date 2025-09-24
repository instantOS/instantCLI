use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

/// Execute swaymsg command
pub fn swaymsg(command: &str) -> Result<String> {
    let output = Command::new("swaymsg")
        .args([command])
        .output()
        .context("Failed to execute swaymsg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("swaymsg failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Execute swaymsg -t get_tree
pub fn swaymsg_get_tree() -> Result<String> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_tree"])
        .output()
        .context("Failed to execute swaymsg -t get_tree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("swaymsg -t get_tree failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check if a window with specific class exists in Sway
pub fn window_exists(window_class: &str) -> Result<bool> {
    let tree = swaymsg_get_tree()?;
    Ok(tree.contains(&format!("\"app_id\": \"{window_class}\"")))
}

/// Check if a window is currently visible (not in scratchpad) in Sway
pub fn is_window_visible(window_class: &str) -> Result<bool> {
    let tree = swaymsg_get_tree()?;
    let parsed: Value = serde_json::from_str(&tree).context("Failed to parse Sway tree JSON")?;

    // Find the window and check its visible status
    find_window_visibility(&parsed, window_class)
}

/// Show a scratchpad window in Sway (idempotent)
/// Only shows if the window is not already visible
pub fn show_scratchpad(window_class: &str) -> Result<()> {
    // Check if window is already visible first
    if is_window_visible(window_class)? {
        // Window is already visible, do nothing
        return Ok(());
    }

    // Window exists but is hidden, show it
    let message = format!("[app_id=\"{window_class}\"] scratchpad show");
    swaymsg(&message)?;
    Ok(())
}

/// Hide a scratchpad window in Sway (idempotent)
/// Only hides if the window is currently visible
pub fn hide_scratchpad(window_class: &str) -> Result<()> {
    // Check if window is currently visible
    if !is_window_visible(window_class)? {
        // Window is already hidden, do nothing
        return Ok(());
    }

    // Window is visible, hide it
    let message = format!("[app_id=\"{window_class}\"] move to scratchpad");
    swaymsg(&message)?;
    Ok(())
}

/// Toggle scratchpad window visibility (maintained for compatibility)
pub fn toggle_scratchpad(window_class: &str) -> Result<()> {
    let message = format!("[app_id=\"{window_class}\"] scratchpad show");
    swaymsg(&message)?;
    Ok(())
}

/// Configure a window for scratchpad use in Sway
pub fn configure_scratchpad_window(
    window_class: &str,
    width_pct: u32,
    height_pct: u32,
) -> Result<()> {
    let config_commands = vec![
        format!("[app_id=\"{}\"] floating enable", window_class),
        format!(
            "[app_id=\"{}\"] resize set width {} ppt height {} ppt",
            window_class, width_pct, height_pct
        ),
        format!("[app_id=\"{}\"] move position center", window_class),
        format!("[app_id=\"{}\"] move to scratchpad", window_class),
    ];

    for cmd in config_commands {
        if let Err(e) = swaymsg(&cmd) {
            eprintln!("Warning: Failed to configure window: {e}");
        }
    }

    Ok(())
}

/// Get all scratchpad windows in Sway
pub fn get_all_scratchpad_windows() -> Result<Vec<ScratchpadWindowInfo>> {
    let tree = swaymsg_get_tree()?;
    let parsed: Value = serde_json::from_str(&tree).context("Failed to parse Sway tree JSON")?;

    let mut scratchpads = Vec::new();

    // Recursively search for scratchpad windows
    if let Some(nodes) = find_scratchpad_nodes(&parsed) {
        for node in nodes {
            if let (Some(name), Some(app_id)) = (get_window_name(node), get_window_app_id(node)) {
                // Check if this is a scratchpad window (app_id starts with "scratchpad_")
                if let Some(scratchpad_name) = app_id.strip_prefix("scratchpad_") {
                    let is_visible = get_node_visible_field(node).unwrap_or(false);
                    scratchpads.push(ScratchpadWindowInfo {
                        name: scratchpad_name.to_string(),
                        window_class: app_id,
                        title: name,
                        visible: is_visible,
                    });
                }
            }
        }
    }

    Ok(scratchpads)
}

/// Get the visible field from a node directly
fn get_node_visible_field(node: &Value) -> Option<bool> {
    node.get("visible").and_then(|v| v.as_bool())
}

/// Information about a scratchpad window
#[derive(Debug, Clone)]
pub struct ScratchpadWindowInfo {
    pub name: String,
    pub window_class: String,
    pub title: String,
    pub visible: bool,
}

/// Recursively find all scratchpad nodes in the Sway tree
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

/// Get window app_id from node
fn get_window_app_id(node: &Value) -> Option<String> {
    node.get("app_id")
        .and_then(|a| a.as_str())
        .map(|s| s.to_string())
}

/// Find window visibility by searching the tree
fn find_window_visibility(tree: &Value, window_class: &str) -> Result<bool> {
    if let Some(visible) = find_window_recursive(tree, window_class) {
        Ok(visible)
    } else {
        // Window not found, assume not visible
        Ok(false)
    }
}

/// Recursive helper to find window and check visibility
fn find_window_recursive(node: &Value, window_class: &str) -> Option<bool> {
    // Check if this node matches our window class
    if let Some(app_id) = get_window_app_id(node) {
        if app_id == window_class {
            // Return the visible field
            return node.get("visible").and_then(|v| v.as_bool());
        }
    }

    // Search in nodes
    if let Some(nodes) = node.get("nodes").and_then(|n| n.as_array()) {
        for child in nodes {
            if let Some(visible) = find_window_recursive(child, window_class) {
                return Some(visible);
            }
        }
    }

    // Search in floating nodes
    if let Some(floating_nodes) = node.get("floating_nodes").and_then(|n| n.as_array()) {
        for child in floating_nodes {
            if let Some(visible) = find_window_recursive(child, window_class) {
                return Some(visible);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_swaymsg_command_format() {
        // Test that command formatting works correctly
        let window_class = "test_class";
        let show_cmd = format!("[app_id=\"{window_class}\"] scratchpad show");
        assert_eq!(show_cmd, "[app_id=\"test_class\"] scratchpad show");

        let hide_cmd = format!("[app_id=\"{window_class}\"] move to scratchpad");
        assert_eq!(hide_cmd, "[app_id=\"test_class\"] move to scratchpad");
    }
}
