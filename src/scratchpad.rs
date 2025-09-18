use crate::compositor::CompositorType;
use crate::hyprland_ipc;
use anyhow::{Context, Result};
use std::process::Command;

/// Configuration for scratchpad terminal behavior
#[derive(Debug, Clone)]
pub struct ScratchpadConfig {
    /// Window class/app_id for the scratchpad terminal
    pub window_class: String,
    /// Terminal command to launch
    pub terminal_command: String,
    /// Terminal width as percentage of screen
    pub width_pct: u32,
    /// Terminal height as percentage of screen
    pub height_pct: u32,
}

impl Default for ScratchpadConfig {
    fn default() -> Self {
        Self {
            window_class: "scratchpad_term".to_string(),
            terminal_command: "kitty".to_string(),
            width_pct: 50,
            height_pct: 60,
        }
    }
}

/// Execute swaymsg command
fn swaymsg(command: &str) -> Result<String> {
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
fn swaymsg_get_tree() -> Result<String> {
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



/// Toggle scratchpad terminal visibility
pub fn toggle_scratchpad(compositor: &CompositorType, config: &ScratchpadConfig) -> Result<()> {
    match compositor {
        CompositorType::Sway => toggle_scratchpad_sway(config),
        CompositorType::Hyprland => toggle_scratchpad_hyprland(config),
        CompositorType::Other(name) => {
            eprintln!("TODO: Scratchpad not yet implemented for compositor: {}", name);
            eprintln!("Currently supported: Sway, Hyprland");
            Ok(())
        }
    }
}

/// Toggle scratchpad terminal for Sway
fn toggle_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let tree = swaymsg_get_tree()?;
    let window_exists = tree.contains(&format!("\"app_id\": \"{}\"", config.window_class));

    if window_exists {
        // Terminal exists, toggle its visibility
        let message = format!("[app_id=\"{}\"] scratchpad show", config.window_class);
        swaymsg(&message)?;
        println!("Toggled scratchpad terminal visibility");
    } else {
        // Terminal doesn't exist, create and configure it
        create_and_configure_sway_scratchpad(config)?;

        // Show it immediately
        let show_message = format!("[app_id=\"{}\"] scratchpad show", config.window_class);
        if let Err(e) = swaymsg(&show_message) {
            eprintln!("Warning: Failed to show scratchpad: {}", e);
        }

        println!("Scratchpad terminal created and configured");
    }

    Ok(())
}

/// Toggle scratchpad terminal for Hyprland
fn toggle_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = "instantscratchpad";

    // Check if terminal with specific class exists using direct IPC
    let window_exists = hyprland_ipc::window_exists(&config.window_class)?;

    if window_exists {
        // Terminal exists, toggle special workspace visibility using direct IPC
        hyprland_ipc::toggle_special_workspace(workspace_name)?;
        println!("Toggled scratchpad terminal visibility");
    } else {
        // Terminal doesn't exist, create it with proper rules
        println!("Creating new scratchpad terminal...");

        // Setup window rules first using direct IPC
        hyprland_ipc::setup_window_rules(workspace_name, &config.window_class)?;

        // Prepare terminal command with appropriate class
        let term_cmd = get_terminal_command_with_class(config);

        // Launch terminal in background using nohup and background operator
        let bg_cmd = format!("nohup {} >/dev/null 2>&1 &", term_cmd);

        Command::new("sh")
            .args(["-c", &bg_cmd])
            .output()
            .context("Failed to launch terminal in background")?;

        // Wait for window to appear
        std::thread::sleep(std::time::Duration::from_millis(500));

        println!("Scratchpad terminal created with window rules");
    }

    Ok(())
}

/// Check if scratchpad terminal is currently visible
pub fn is_scratchpad_visible(compositor: &CompositorType, config: &ScratchpadConfig) -> Result<bool> {
    match compositor {
        CompositorType::Sway => {
            let tree = swaymsg_get_tree()?;
            // Look for the window and check if it's visible (not in scratchpad)
            Ok(tree.contains(&format!("\"app_id\": \"{}\"", config.window_class))
               && !tree.contains(&format!("\"app_id\": \"{}\".*scratchpad", config.window_class)))
        }
        CompositorType::Hyprland => {
            // For Hyprland, check if special workspace is active AND window exists using direct IPC
            let workspace_name = "instantscratchpad";

            // Check if special workspace is active using direct IPC
            let special_workspace_active = hyprland_ipc::is_special_workspace_active(workspace_name).unwrap_or(false);

            // Check if window exists using direct IPC
            let window_exists = hyprland_ipc::window_exists(&config.window_class).unwrap_or(false);

            Ok(special_workspace_active && window_exists)
        }
        CompositorType::Other(_) => {
            eprintln!("TODO: Scratchpad visibility check not implemented for this compositor");
            Ok(false)
        }
    }
}

/// Show scratchpad terminal (create if it doesn't exist)
pub fn show_scratchpad(compositor: &CompositorType, config: &ScratchpadConfig) -> Result<()> {
    match compositor {
        CompositorType::Sway => show_scratchpad_sway(config),
        CompositorType::Hyprland => show_scratchpad_hyprland(config),
        CompositorType::Other(name) => {
            eprintln!("TODO: Scratchpad show not yet implemented for compositor: {}", name);
            eprintln!("Currently supported: Sway, Hyprland");
            Ok(())
        }
    }
}

/// Hide scratchpad terminal
pub fn hide_scratchpad(compositor: &CompositorType, config: &ScratchpadConfig) -> Result<()> {
    match compositor {
        CompositorType::Sway => hide_scratchpad_sway(config),
        CompositorType::Hyprland => hide_scratchpad_hyprland(config),
        CompositorType::Other(name) => {
            eprintln!("TODO: Scratchpad hide not yet implemented for compositor: {}", name);
            eprintln!("Currently supported: Sway, Hyprland");
            Ok(())
        }
    }
}

/// Show scratchpad terminal for Sway
fn show_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let tree = swaymsg_get_tree()?;
    let window_exists = tree.contains(&format!("\"app_id\": \"{}\"", config.window_class));

    if window_exists {
        // Terminal exists, show it
        let message = format!("[app_id=\"{}\"] scratchpad show", config.window_class);
        swaymsg(&message)?;
        println!("Showed scratchpad terminal");
    } else {
        // Terminal doesn't exist, create and configure it
        create_and_configure_sway_scratchpad(config)?;

        // Show it immediately
        let show_message = format!("[app_id=\"{}\"] scratchpad show", config.window_class);
        if let Err(e) = swaymsg(&show_message) {
            eprintln!("Warning: Failed to show scratchpad: {}", e);
        }

        println!("Scratchpad terminal created and shown");
    }

    Ok(())
}

/// Hide scratchpad terminal for Sway
fn hide_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let tree = swaymsg_get_tree()?;
    let window_exists = tree.contains(&format!("\"app_id\": \"{}\"", config.window_class));

    if window_exists {
        // Check if it's currently visible (not in scratchpad)
        let is_visible = !tree.contains(&format!("\"app_id\": \"{}\".*scratchpad", config.window_class));

        if is_visible {
            // Terminal is visible, move it to scratchpad (hide it)
            let message = format!("[app_id=\"{}\"] move to scratchpad", config.window_class);
            swaymsg(&message)?;
            println!("Hid scratchpad terminal");
        } else {
            println!("Scratchpad terminal is already hidden");
        }
    } else {
        println!("Scratchpad terminal does not exist");
    }

    Ok(())
}

/// Show scratchpad terminal for Hyprland
fn show_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = "instantscratchpad";

    // Check if terminal with specific class exists using direct IPC
    let window_exists = hyprland_ipc::window_exists(&config.window_class)?;

    if window_exists {
        // Terminal exists, show special workspace using direct IPC
        hyprland_ipc::show_special_workspace(workspace_name)?;
        println!("Showed scratchpad terminal");
    } else {
        // Terminal doesn't exist, create it with proper rules
        println!("Creating new scratchpad terminal...");

        // Setup window rules first using direct IPC
        hyprland_ipc::setup_window_rules(workspace_name, &config.window_class)?;

        // Prepare terminal command with appropriate class
        let term_cmd = get_terminal_command_with_class(config);

        // Launch terminal in background using nohup and background operator
        let bg_cmd = format!("nohup {} >/dev/null 2>&1 &", term_cmd);

        Command::new("sh")
            .args(["-c", &bg_cmd])
            .output()
            .context("Failed to launch terminal in background")?;

        // Wait a moment for the window to appear and be configured
        std::thread::sleep(std::time::Duration::from_millis(1000));

        // Show the special workspace using direct IPC
        hyprland_ipc::show_special_workspace(workspace_name)?;

        println!("Scratchpad terminal created and shown");
    }

    Ok(())
}

/// Hide scratchpad terminal for Hyprland
fn hide_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = "instantscratchpad";

    // Check if terminal with specific class exists using direct IPC
    let window_exists = hyprland_ipc::window_exists(&config.window_class)?;

    if window_exists {
        // Check if special workspace is currently active
        let is_visible = hyprland_ipc::is_special_workspace_active(workspace_name).unwrap_or(false);

        if is_visible {
            // Terminal is visible, hide special workspace using direct IPC
            hyprland_ipc::hide_special_workspace(workspace_name)?;
            println!("Hid scratchpad terminal");
        } else {
            println!("Scratchpad terminal is already hidden");
        }
    } else {
        println!("Scratchpad terminal does not exist");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ScratchpadConfig::default();
        assert_eq!(config.window_class, "scratchpad_term");
        assert_eq!(config.terminal_command, "kitty");
        assert_eq!(config.width_pct, 50);
        assert_eq!(config.height_pct, 60);
    }

    #[test]
    fn test_custom_config() {
        let config = ScratchpadConfig {
            window_class: "my_scratch".to_string(),
            terminal_command: "kitty".to_string(),
            width_pct: 70,
            height_pct: 80,
        };

        assert_eq!(config.window_class, "my_scratch");
        assert_eq!(config.terminal_command, "kitty");
        assert_eq!(config.width_pct, 70);
        assert_eq!(config.height_pct, 80);
    }

    #[test]
    fn test_get_terminal_command_with_class() {
        let config = ScratchpadConfig {
            window_class: "test_class".to_string(),
            terminal_command: "alacritty".to_string(),
            width_pct: 50,
            height_pct: 60,
        };

        let cmd = get_terminal_command_with_class(&config);
        assert_eq!(cmd, "alacritty --class test_class");

        let config_kitty = ScratchpadConfig {
            window_class: "test_class".to_string(),
            terminal_command: "kitty".to_string(),
            width_pct: 50,
            height_pct: 60,
        };

        let cmd_kitty = get_terminal_command_with_class(&config_kitty);
        assert_eq!(cmd_kitty, "kitty --class test_class");

        let config_other = ScratchpadConfig {
            window_class: "test_class".to_string(),
            terminal_command: "wezterm".to_string(),
            width_pct: 50,
            height_pct: 60,
        };

        let cmd_other = get_terminal_command_with_class(&config_other);
        assert_eq!(cmd_other, "wezterm");
    }

    #[test]
    fn test_show_hide_functions_exist() {
        // Test that the public API functions exist and can be called
        // This is a basic smoke test to ensure the functions are properly exposed
        use crate::compositor::CompositorType;

        let config = ScratchpadConfig::default();
        let compositor = CompositorType::Other("test".to_string());

        // These should not panic and should return Ok(()) for unsupported compositor
        let show_result = show_scratchpad(&compositor, &config);
        let hide_result = hide_scratchpad(&compositor, &config);
        let visible_result = is_scratchpad_visible(&compositor, &config);

        assert!(show_result.is_ok());
        assert!(hide_result.is_ok());
        assert!(visible_result.is_ok());
        assert_eq!(visible_result.unwrap(), false); // Should return false for unsupported compositor
    }
}

/// Create and configure a new scratchpad terminal for Sway
fn create_and_configure_sway_scratchpad(config: &ScratchpadConfig) -> Result<()> {
    println!("Creating new scratchpad terminal...");

    // Launch the terminal in background
    let term_cmd = get_terminal_command_with_class(config);

    // Launch terminal in background using nohup and background operator
    // This ensures the terminal continues running after our command exits
    let bg_cmd = format!("nohup {} >/dev/null 2>&1 &", term_cmd);

    Command::new("sh")
        .args(["-c", &bg_cmd])
        .output()
        .context("Failed to launch terminal in background")?;

    // Wait a moment for the window to appear
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Configure the new window
    let config_commands = vec![
        format!("[app_id=\"{}\"] floating enable", config.window_class),
        format!("[app_id=\"{}\"] resize set width {} ppt height {} ppt",
               config.window_class, config.width_pct, config.height_pct),
        format!("[app_id=\"{}\"] move position center", config.window_class),
        format!("[app_id=\"{}\"] move to scratchpad", config.window_class),
    ];

    for cmd in config_commands {
        if let Err(e) = swaymsg(&cmd) {
            eprintln!("Warning: Failed to configure window: {}", e);
            // Don't fail completely if configuration fails
        }
    }

    Ok(())
}

/// Get terminal command with appropriate class flag
fn get_terminal_command_with_class(config: &ScratchpadConfig) -> String {
    // Add class flag for supported terminals
    match config.terminal_command.as_str() {
        "alacritty" => format!("{} --class {}", config.terminal_command, config.window_class),
        "kitty" => format!("{} --class {}", config.terminal_command, config.window_class),
        _ => config.terminal_command.clone(),
    }
}

/// Handle scratchpad commands
pub fn handle_scratchpad_command(command: ScratchpadCommands, debug: bool) -> Result<i32> {
    if debug {
        eprintln!("Debug mode is on for scratchpad command");
    }

    // Detect current compositor
    let compositor = CompositorType::detect();
    if debug {
        eprintln!("Detected compositor: {}", compositor.name());
    }

    match command {
        ScratchpadCommands::Toggle {
            window_class,
            terminal,
            width_pct,
            height_pct,
        } => {
            if debug {
                eprintln!("Toggle scratchpad with config:");
                eprintln!("  window_class: {}", window_class);
                eprintln!("  terminal: {}", terminal);
                eprintln!("  width_pct: {}", width_pct);
                eprintln!("  height_pct: {}", height_pct);
            }

            let config = ScratchpadConfig {
                window_class,
                terminal_command: terminal,
                width_pct,
                height_pct,
            };

            match toggle_scratchpad(&compositor, &config) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("Error toggling scratchpad: {}", e);
                    Ok(1)
                }
            }
        }
        ScratchpadCommands::Show {
            window_class,
            terminal,
            width_pct,
            height_pct,
        } => {
            if debug {
                eprintln!("Show scratchpad with config:");
                eprintln!("  window_class: {}", window_class);
                eprintln!("  terminal: {}", terminal);
                eprintln!("  width_pct: {}", width_pct);
                eprintln!("  height_pct: {}", height_pct);
            }

            let config = ScratchpadConfig {
                window_class,
                terminal_command: terminal,
                width_pct,
                height_pct,
            };

            match show_scratchpad(&compositor, &config) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("Error showing scratchpad: {}", e);
                    Ok(1)
                }
            }
        }
        ScratchpadCommands::Hide { window_class } => {
            if debug {
                eprintln!("Hide scratchpad for: {}", window_class);
            }

            let config = ScratchpadConfig {
                window_class,
                ..Default::default()
            };

            match hide_scratchpad(&compositor, &config) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("Error hiding scratchpad: {}", e);
                    Ok(1)
                }
            }
        }
        ScratchpadCommands::Status { window_class } => {
            if debug {
                eprintln!("Check scratchpad status for: {}", window_class);
            }

            let config = ScratchpadConfig {
                window_class,
                ..Default::default()
            };

            match is_scratchpad_visible(&compositor, &config) {
                Ok(visible) => {
                    if visible {
                        println!("Scratchpad terminal is visible");
                        Ok(0)
                    } else {
                        println!("Scratchpad terminal is not visible");
                        Ok(1)
                    }
                }
                Err(e) => {
                    eprintln!("Error checking scratchpad status: {}", e);
                    Ok(2)
                }
            }
        }
    }
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum ScratchpadCommands {
    /// Toggle scratchpad terminal visibility
    Toggle {
        /// Window class/app_id for the scratchpad terminal
        #[arg(long, default_value = "scratchpad_term")]
        window_class: String,
        /// Terminal command to launch
        #[arg(long, default_value = "kitty")]
        terminal: String,
        /// Terminal width as percentage of screen
        #[arg(long, default_value = "50")]
        width_pct: u32,
        /// Terminal height as percentage of screen
        #[arg(long, default_value = "60")]
        height_pct: u32,
    },
    /// Show scratchpad terminal (create if it doesn't exist)
    Show {
        /// Window class/app_id for the scratchpad terminal
        #[arg(long, default_value = "scratchpad_term")]
        window_class: String,
        /// Terminal command to launch
        #[arg(long, default_value = "kitty")]
        terminal: String,
        /// Terminal width as percentage of screen
        #[arg(long, default_value = "50")]
        width_pct: u32,
        /// Terminal height as percentage of screen
        #[arg(long, default_value = "60")]
        height_pct: u32,
    },
    /// Hide scratchpad terminal
    Hide {
        /// Window class/app_id for the scratchpad terminal
        #[arg(long, default_value = "scratchpad_term")]
        window_class: String,
    },
    /// Check if scratchpad terminal is currently visible
    Status {
        /// Window class/app_id for the scratchpad terminal
        #[arg(long, default_value = "scratchpad_term")]
        window_class: String,
    },
}
