use super::config::ScratchpadConfig;
use crate::common::compositor::{CompositorType, hyprland, sway};
use anyhow::{Context, Result};
use std::process::Command;

/// Create and launch terminal in background
fn create_terminal_process(config: &ScratchpadConfig) -> Result<()> {
    let term_cmd = config.terminal_command();
    let bg_cmd = format!("nohup {term_cmd} >/dev/null 2>&1 &");

    Command::new("sh")
        .args(["-c", &bg_cmd])
        .output()
        .context("Failed to launch terminal in background")?;

    Ok(())
}

/// Check if a window exists using the appropriate compositor method
pub fn check_window_exists(compositor: &CompositorType, window_class: &str) -> Result<bool> {
    match compositor {
        CompositorType::Sway => sway::window_exists(window_class),
        CompositorType::Hyprland => Ok(hyprland::window_exists(window_class).unwrap_or(false)),
        CompositorType::Other(_) => {
            // For unsupported compositors, assume window doesn't exist
            Ok(false)
        }
    }
}

/// Wait for a window to appear by polling the compositor
/// Returns true if window appeared, false if timeout reached
pub fn wait_for_window_to_appear(
    compositor: &CompositorType,
    window_class: &str,
    max_attempts: u32,
    poll_interval_ms: u64,
) -> Result<bool> {
    for attempt in 1..=max_attempts {
        // Small delay before checking
        std::thread::sleep(std::time::Duration::from_millis(poll_interval_ms));

        let window_exists = check_window_exists(compositor, window_class)?;

        if window_exists {
            return Ok(true);
        }

        if attempt < max_attempts {
            eprintln!("Waiting for window to appear... (attempt {attempt}/{max_attempts})");
        }
    }

    Ok(false) // Timeout reached
}

/// Create and configure a new scratchpad terminal for Sway
pub fn create_and_configure_sway_scratchpad(config: &ScratchpadConfig) -> Result<()> {
    println!("Creating new scratchpad terminal '{}'...", config.name);

    // Launch the terminal in background
    create_terminal_process(config)?;

    // Wait for the window to appear by polling
    let window_class = config.window_class();
    let window_appeared = wait_for_window_to_appear(
        &CompositorType::Sway,
        &window_class,
        20,  // max attempts
        100, // poll every 100ms
    )?;

    if !window_appeared {
        return Err(anyhow::anyhow!(
            "Terminal window did not appear after waiting. The terminal command may have failed to start."
        ));
    }

    // Configure the new window
    std::thread::sleep(std::time::Duration::from_millis(200));
    sway::configure_scratchpad_window(&window_class, config.width_pct, config.height_pct)?;

    Ok(())
}

/// Create and configure a new scratchpad terminal for Hyprland
pub fn create_and_configure_hyprland_scratchpad(config: &ScratchpadConfig) -> Result<()> {
    println!("Creating new scratchpad terminal '{}'...", config.name);

    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Setup window rules first using direct IPC
    hyprland::setup_window_rules(&workspace_name, &window_class)?;

    // Launch the terminal in background
    create_terminal_process(config)?;

    // Wait for the window to appear by polling
    let window_appeared = wait_for_window_to_appear(
        &CompositorType::Hyprland,
        &window_class,
        20,  // max attempts
        100, // poll every 100ms
    )?;

    if !window_appeared {
        return Err(anyhow::anyhow!(
            "Terminal window did not appear after waiting. The terminal command may have failed to start."
        ));
    }

    Ok(())
}

/// Toggle scratchpad terminal for Sway
pub fn toggle_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let window_class = config.window_class();
    let window_exists = check_window_exists(&CompositorType::Sway, &window_class)?;

    if window_exists {
        // Terminal exists, toggle its visibility
        sway::show_scratchpad(&window_class)?;
        println!("Toggled scratchpad terminal '{}' visibility", config.name);
    } else {
        // Terminal doesn't exist, create and configure it
        create_and_configure_sway_scratchpad(config)?;

        // Show it immediately (toggle means show when creating)
        sway::show_scratchpad(&window_class)?;

        println!("Scratchpad terminal '{}' created and shown", config.name);
    }

    Ok(())
}

/// Toggle scratchpad terminal for Hyprland
pub fn toggle_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Check if terminal with specific class exists using direct IPC
    let window_exists = check_window_exists(&CompositorType::Hyprland, &window_class)?;

    if window_exists {
        // Terminal exists, toggle special workspace visibility using direct IPC
        hyprland::toggle_special_workspace(&workspace_name)?;
        println!("Toggled scratchpad terminal '{}' visibility", config.name);
    } else {
        // Terminal doesn't exist, create it with proper rules
        create_and_configure_hyprland_scratchpad(config)?;

        // Show it immediately (toggle means show when creating)
        hyprland::show_special_workspace(&workspace_name)?;

        println!("Scratchpad terminal '{}' created and shown", config.name);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_window_exists() {
        // Test with unsupported compositor
        let result = check_window_exists(&CompositorType::Other("test".to_string()), "test_window");
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should return false for unsupported compositor
    }

    #[test]
    fn test_wait_for_window_polling() {
        // Test with unsupported compositor (should return false after max attempts since window doesn't exist)
        let result = wait_for_window_to_appear(
            &CompositorType::Other("test".to_string()),
            "test_window",
            2,  // max attempts
            10, // poll interval (short for test)
        );

        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should return false for unsupported compositor (window doesn't exist)
    }
}
