use anyhow::{Context, Result};
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
    Ok(tree.contains(&format!("\"app_id\": \"{}\"", window_class)))
}

/// Check if a window is currently visible (not in scratchpad) in Sway
pub fn is_window_visible(window_class: &str) -> Result<bool> {
    let tree = swaymsg_get_tree()?;
    // Look for the window and check if it's visible (not in scratchpad)
    Ok(tree.contains(&format!("\"app_id\": \"{}\"", window_class))
        && !tree.contains(&format!("\"app_id\": \"{}\".*scratchpad", window_class)))
}

/// Show a scratchpad window in Sway
pub fn show_scratchpad(window_class: &str) -> Result<()> {
    let message = format!("[app_id=\"{}\"] scratchpad show", window_class);
    swaymsg(&message)?;
    Ok(())
}

/// Hide a scratchpad window in Sway
pub fn hide_scratchpad(window_class: &str) -> Result<()> {
    let message = format!("[app_id=\"{}\"] move to scratchpad", window_class);
    swaymsg(&message)?;
    Ok(())
}

/// Configure a window for scratchpad use in Sway
pub fn configure_scratchpad_window(window_class: &str, width_pct: u32, height_pct: u32) -> Result<()> {
    let config_commands = vec![
        format!("[app_id=\"{}\"] floating enable", window_class),
        format!("[app_id=\"{}\"] resize set width {} ppt height {} ppt",
               window_class, width_pct, height_pct),
        format!("[app_id=\"{}\"] move position center", window_class),
        format!("[app_id=\"{}\"] move to scratchpad", window_class),
    ];

    for cmd in config_commands {
        if let Err(e) = swaymsg(&cmd) {
            eprintln!("Warning: Failed to configure window: {}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swaymsg_command_format() {
        // Test that command formatting works correctly
        let window_class = "test_class";
        let show_cmd = format!("[app_id=\"{}\"] scratchpad show", window_class);
        assert_eq!(show_cmd, "[app_id=\"test_class\"] scratchpad show");
        
        let hide_cmd = format!("[app_id=\"{}\"] move to scratchpad", window_class);
        assert_eq!(hide_cmd, "[app_id=\"test_class\"] move to scratchpad");
    }
}
