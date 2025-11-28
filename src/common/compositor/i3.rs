use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

/// Execute i3-msg command
pub fn i3msg(command: &str) -> Result<String> {
    let output = Command::new("i3-msg")
        .args([command])
        .output()
        .context("Failed to execute i3-msg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("i3-msg failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Execute i3-msg -t get_tree
pub fn i3msg_get_tree() -> Result<String> {
    let output = Command::new("i3-msg")
        .args(["-t", "get_tree"])
        .output()
        .context("Failed to execute i3-msg -t get_tree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("i3-msg -t get_tree failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check if a window with specific class exists in i3
pub fn window_exists(window_class: &str) -> Result<bool> {
    let tree = i3msg_get_tree()?;
    Ok(tree.contains(&format!("\"class\": \"{}\"", window_class))
        || tree.contains(&format!("\"instance\": \"{}\"", window_class)))
}

/// Check if a window is currently visible (not in scratchpad) in i3
pub fn is_window_visible(window_class: &str) -> Result<bool> {
    let tree = i3msg_get_tree()?;
    let parsed: Value = serde_json::from_str(&tree).context("Failed to parse i3 tree JSON")?;

    // Find the window and check its visible status
    find_window_visibility(&parsed, window_class)
}

/// Show a scratchpad window in i3 (idempotent)
/// Only shows if the window is not already visible
pub fn show_scratchpad(window_class: &str) -> Result<()> {
    // Check if window is already visible first
    if is_window_visible(window_class)? {
        // Window is already visible, do nothing
        return Ok(());
    }

    // Window exists but is hidden, show it
    let message = format!("[class=\"{}\"] scratchpad show", window_class);
    i3msg(&message)?;
    Ok(())
}

/// Hide a scratchpad window in i3 (idempotent)
/// Only hides if the window is currently visible
pub fn hide_scratchpad(window_class: &str) -> Result<()> {
    // Check if window is currently visible
    if !is_window_visible(window_class)? {
        // Window is already hidden, do nothing
        return Ok(());
    }

    // Window is visible, hide it
    let message = format!("[class=\"{}\"] move scratchpad", window_class);
    i3msg(&message)?;
    Ok(())
}

/// Toggle scratchpad window visibility (maintained for compatibility)
pub fn toggle_scratchpad(window_class: &str) -> Result<()> {
    let message = format!("[class=\"{}\"] scratchpad show", window_class);
    i3msg(&message)?;
    Ok(())
}

/// Configure a window for scratchpad use in i3
pub fn configure_scratchpad_window(
    window_class: &str,
    width_pct: u32,
    height_pct: u32,
) -> Result<()> {
    let config_commands = vec![
        format!("[class=\"{}\"] floating enable", window_class),
        format!(
            "[class=\"{}\"] resize set width {} ppt height {} ppt",
            window_class, width_pct, height_pct
        ),
        format!("[class=\"{}\"] move position center", window_class),
        format!("[class=\"{}\"] move scratchpad", window_class),
    ];

    for cmd in config_commands {
        if let Err(e) = i3msg(&cmd) {
            eprintln!("Warning: Failed to configure window: {e}");
        }
    }

    Ok(())
}

/// Get all scratchpad windows in i3
pub fn get_all_scratchpad_windows() -> Result<Vec<ScratchpadWindowInfo>> {
    let tree = i3msg_get_tree()?;
    let parsed: Value = serde_json::from_str(&tree).context("Failed to parse i3 tree JSON")?;

    let mut scratchpads = Vec::new();

    // Recursively search for scratchpad windows
    if let Some(nodes) = find_scratchpad_nodes(&parsed) {
        for node in nodes {
            if let (Some(name), Some(class)) = (get_window_name(node), get_window_class(node)) {
                // Check if this is a scratchpad window (class starts with "scratchpad_")
                if let Some(scratchpad_name) = class.strip_prefix("scratchpad_") {
                    let is_visible = get_node_visible_field(node).unwrap_or(false);
                    scratchpads.push(ScratchpadWindowInfo {
                        name: scratchpad_name.to_string(),
                        window_class: class,
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

/// Recursively find all scratchpad nodes in the i3 tree
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

/// Get window class from node
fn get_window_class(node: &Value) -> Option<String> {
    node.get("window_properties")
        .and_then(|wp| wp.get("class"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // Fallback to instance if class is not available
            node.get("window_properties")
                .and_then(|wp| wp.get("instance"))
                .and_then(|i| i.as_str())
                .map(|s| s.to_string())
        })
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
    if let Some(class) = get_window_class(node)
        && class == window_class
    {
        // Return the visible field
        return node.get("visible").and_then(|v| v.as_bool());
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
    use super::*;

    #[test]
    fn test_i3msg_command_format() {
        // Test that command formatting works correctly
        let window_class = "test_class";
        let show_cmd = format!("[class=\"{}\"] scratchpad show", window_class);
        assert_eq!(show_cmd, "[class=\"test_class\"] scratchpad show");

        let hide_cmd = format!("[class=\"{}\"] move scratchpad", window_class);
        assert_eq!(hide_cmd, "[class=\"test_class\"] move scratchpad");
    }
}