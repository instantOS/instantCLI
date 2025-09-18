use crate::compositor::CompositorType;
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
            terminal_command: "alacritty".to_string(),
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

/// Execute hyprctl command
fn hyprctl(command: &str) -> Result<String> {
    let output = Command::new("hyprctl")
        .args([command])
        .output()
        .context("Failed to execute hyprctl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("hyprctl failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Execute hyprctl clients
fn hyprctl_clients() -> Result<String> {
    let output = Command::new("hyprctl")
        .args(["clients"])
        .output()
        .context("Failed to execute hyprctl clients")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("hyprctl clients failed: {}", stderr);
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
        println!("Creating new scratchpad terminal...");

        // Launch the terminal in background
        let mut term_cmd = config.terminal_command.clone();
        if config.terminal_command == "alacritty" {
            term_cmd = format!("{} --class {}", config.terminal_command, config.window_class);
        }

        // Launch terminal in background using nohup and background operator
        // This ensures the terminal continues running after our command exits
        // TODO: this only needs linux support, remove the other thing
        let bg_cmd = if cfg!(unix) {
            format!("nohup {} >/dev/null 2>&1 &", term_cmd)
        } else {
            format!("start /b {}", term_cmd)
        };

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
    // For Hyprland, we'll use special workspaces approach
    // Check if terminal exists by looking at clients
    let clients = hyprctl_clients()?;
    let window_exists = clients.contains(&config.window_class);

    if window_exists {
        // Terminal exists, toggle to/from special workspace
        hyprctl("dispatch togglespecialworkspace scratchpad")?;
        println!("Toggled scratchpad terminal visibility");
    } else {
        // Terminal doesn't exist, create it on special workspace
        println!("Creating new scratchpad terminal...");

        // Launch the terminal with special workspace rules
        let mut term_cmd = config.terminal_command.clone();
        if config.terminal_command == "alacritty" {
            term_cmd = format!("{} --class {}", config.terminal_command, config.window_class);
        }

        // First move to special workspace
        hyprctl("dispatch workspace special:scratchpad")?;

        // Launch terminal in background using nohup and background operator
        let bg_cmd = if cfg!(unix) {
            format!("nohup {} >/dev/null 2>&1 &", term_cmd)
        } else {
            format!("start /b {}", term_cmd)
        };

        Command::new("sh")
            .args(["-c", &bg_cmd])
            .output()
            .context("Failed to launch terminal in background")?;

        // Wait for window to appear
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Configure window (hyprctl syntax differs)
        let config_commands = vec![
            format!("dispatch floating active, class:{}", config.window_class),
            format!("dispatch resize {}% {}%, class:{}", config.width_pct, config.height_pct, config.window_class),
            format!("dispatch centerwindow, class:{}", config.window_class),
        ];

        for cmd in config_commands {
            if let Err(e) = hyprctl(&cmd) {
                eprintln!("Warning: Failed to configure window: {}", e);
                // Don't fail completely if configuration fails
            }
        }

        println!("Scratchpad terminal created on special workspace");
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
            // For Hyprland, check if special workspace is active
            let activeworkspace = Command::new("hyprctl")
                .args(["activeworkspace"])
                .output()
                .context("Failed to get active workspace")?;

            let output = String::from_utf8_lossy(&activeworkspace.stdout);
            Ok(output.contains("special:scratchpad"))
        }
        CompositorType::Other(_) => {
            eprintln!("TODO: Scratchpad visibility check not implemented for this compositor");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ScratchpadConfig::default();
        assert_eq!(config.window_class, "scratchpad_term");
        assert_eq!(config.terminal_command, "alacritty");
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
        #[arg(long, default_value = "alacritty")]
        terminal: String,
        /// Terminal width as percentage of screen
        #[arg(long, default_value = "50")]
        width_pct: u32,
        /// Terminal height as percentage of screen
        #[arg(long, default_value = "60")]
        height_pct: u32,
    },
    /// Check if scratchpad terminal is currently visible
    Status {
        /// Window class/app_id for the scratchpad terminal
        #[arg(long, default_value = "scratchpad_term")]
        window_class: String,
    },
}
