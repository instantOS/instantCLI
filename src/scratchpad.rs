use crate::compositor::CompositorType;
use crate::hyprland_ipc;
use anyhow::{Context, Result};
use std::process::Command;

/// Configuration for scratchpad terminal behavior
#[derive(Debug, Clone)]
pub struct ScratchpadConfig {
    /// Scratchpad name (used as prefix for window class)
    pub name: String,
    /// Terminal command to launch
    pub terminal_command: String,
    /// Command to run inside the terminal (optional)
    pub inner_command: Option<String>,
    /// Terminal width as percentage of screen
    pub width_pct: u32,
    /// Terminal height as percentage of screen
    pub height_pct: u32,
}

impl ScratchpadConfig {
    /// Create a new scratchpad config with the given name
    pub fn new(name: String) -> Self {
        Self {
            name,
            terminal_command: "kitty".to_string(),
            inner_command: None,
            width_pct: 50,
            height_pct: 60,
        }
    }

    /// Create config with custom parameters
    pub fn with_params(
        name: String,
        terminal: String,
        inner_command: Option<String>,
        width_pct: u32,
        height_pct: u32,
    ) -> Self {
        Self {
            name,
            terminal_command: terminal,
            inner_command,
            width_pct,
            height_pct,
        }
    }

    /// Get the window class/app_id for this scratchpad
    pub fn window_class(&self) -> String {
        format!("scratchpad_{}", self.name)
    }

    /// Get the workspace name for this scratchpad (used by Hyprland)
    pub fn workspace_name(&self) -> String {
        format!("scratchpad_{}", self.name)
    }
}

impl Default for ScratchpadConfig {
    fn default() -> Self {
        Self::new("instantscratchpad".to_string())
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
            eprintln!(
                "TODO: Scratchpad not yet implemented for compositor: {}",
                name
            );
            eprintln!("Currently supported: Sway, Hyprland");
            Ok(())
        }
    }
}

/// Toggle scratchpad terminal for Sway
fn toggle_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let window_class = config.window_class();
    let window_exists = check_window_exists(&CompositorType::Sway, &window_class)?;

    if window_exists {
        // Terminal exists, toggle its visibility
        let message = format!("[app_id=\"{}\"] scratchpad show", window_class);
        swaymsg(&message)?;
        println!("Toggled scratchpad terminal '{}' visibility", config.name);
    } else {
        // Terminal doesn't exist, create and configure it
        create_and_configure_sway_scratchpad(config)?;

        // Show it immediately (toggle means show when creating)
        let show_message = format!("[app_id=\"{}\"] scratchpad show", window_class);
        if let Err(e) = swaymsg(&show_message) {
            eprintln!("Warning: Failed to show scratchpad: {}", e);
        }

        println!(
            "Scratchpad terminal '{}' created and shown",
            config.name
        );
    }

    Ok(())
}

/// Toggle scratchpad terminal for Hyprland
fn toggle_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Check if terminal with specific class exists using direct IPC
    let window_exists = check_window_exists(&CompositorType::Hyprland, &window_class)?;

    if window_exists {
        // Terminal exists, toggle special workspace visibility using direct IPC
        hyprland_ipc::toggle_special_workspace(&workspace_name)?;
        println!("Toggled scratchpad terminal '{}' visibility", config.name);
    } else {
        // Terminal doesn't exist, create it with proper rules
        create_and_configure_hyprland_scratchpad(config)?;

        // Show it immediately (toggle means show when creating)
        hyprland_ipc::show_special_workspace(&workspace_name)?;

        println!(
            "Scratchpad terminal '{}' created and shown",
            config.name
        );
    }

    Ok(())
}

/// Check if scratchpad terminal is currently visible
pub fn is_scratchpad_visible(
    compositor: &CompositorType,
    config: &ScratchpadConfig,
) -> Result<bool> {
    let window_class = config.window_class();
    match compositor {
        CompositorType::Sway => {
            let tree = swaymsg_get_tree()?;
            // Look for the window and check if it's visible (not in scratchpad)
            Ok(tree.contains(&format!("\"app_id\": \"{}\"", window_class))
                && !tree.contains(&format!("\"app_id\": \"{}\".*scratchpad", window_class)))
        }
        CompositorType::Hyprland => {
            // For Hyprland, check if special workspace is active AND window exists using direct IPC
            let workspace_name = config.workspace_name();

            // Check if special workspace is active using direct IPC
            let special_workspace_active =
                hyprland_ipc::is_special_workspace_active(&workspace_name).unwrap_or(false);

            // Check if window exists using direct IPC
            let window_exists = check_window_exists(compositor, &window_class)?;

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
            eprintln!(
                "TODO: Scratchpad show not yet implemented for compositor: {}",
                name
            );
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
            eprintln!(
                "TODO: Scratchpad hide not yet implemented for compositor: {}",
                name
            );
            eprintln!("Currently supported: Sway, Hyprland");
            Ok(())
        }
    }
}

/// Create and launch terminal in background
fn create_terminal_process(config: &ScratchpadConfig) -> Result<()> {
    let term_cmd = get_terminal_command_with_class(config);
    let bg_cmd = format!("nohup {} >/dev/null 2>&1 &", term_cmd);

    Command::new("sh")
        .args(["-c", &bg_cmd])
        .output()
        .context("Failed to launch terminal in background")?;

    Ok(())
}

/// Check if a window exists using the appropriate compositor method
fn check_window_exists(compositor: &CompositorType, window_class: &str) -> Result<bool> {
    match compositor {
        CompositorType::Sway => {
            let tree = swaymsg_get_tree()?;
            Ok(tree.contains(&format!("\"app_id\": \"{}\"", window_class)))
        }
        CompositorType::Hyprland => {
            Ok(hyprland_ipc::window_exists(window_class).unwrap_or(false))
        }
        CompositorType::Other(_) => {
            // For unsupported compositors, assume window doesn't exist
            Ok(false)
        }
    }
}

/// Wait for a window to appear by polling the compositor
/// Returns true if window appeared, false if timeout reached
fn wait_for_window_to_appear(
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
            eprintln!(
                "Waiting for window to appear... (attempt {}/{})",
                attempt, max_attempts
            );
        }
    }

    Ok(false) // Timeout reached
}

/// Create and configure a new scratchpad terminal for Sway
fn create_and_configure_sway_scratchpad(config: &ScratchpadConfig) -> Result<()> {
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
    let config_commands = vec![
        format!("[app_id=\"{}\"] floating enable", window_class),
        format!(
            "[app_id=\"{}\"] resize set width {} ppt height {} ppt",
            window_class, config.width_pct, config.height_pct
        ),
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

/// Create and configure a new scratchpad terminal for Hyprland
fn create_and_configure_hyprland_scratchpad(config: &ScratchpadConfig) -> Result<()> {
    println!("Creating new scratchpad terminal '{}'...", config.name);

    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Setup window rules first using direct IPC
    hyprland_ipc::setup_window_rules(&workspace_name, &window_class)?;

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

/// Show scratchpad terminal for Sway
fn show_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let window_class = config.window_class();
    let window_exists = check_window_exists(&CompositorType::Sway, &window_class)?;

    if window_exists {
        // Terminal exists, show it
        let message = format!("[app_id=\"{}\"] scratchpad show", window_class);
        swaymsg(&message)?;
        println!("Showed scratchpad terminal '{}'", config.name);
    } else {
        // Terminal doesn't exist, create and configure it
        create_and_configure_sway_scratchpad(config)?;

        // Show it immediately
        let show_message = format!("[app_id=\"{}\"] scratchpad show", window_class);
        if let Err(e) = swaymsg(&show_message) {
            eprintln!("Warning: Failed to show scratchpad: {}", e);
        }

        println!("Scratchpad terminal '{}' created and shown", config.name);
    }

    Ok(())
}

/// Hide scratchpad terminal for Sway
fn hide_scratchpad_sway(config: &ScratchpadConfig) -> Result<()> {
    // Check if scratchpad terminal exists
    let window_class = config.window_class();
    let window_exists = check_window_exists(&CompositorType::Sway, &window_class)?;

    if window_exists {
        // Check if it's currently visible (not in scratchpad)
        let tree = swaymsg_get_tree()?;
        let is_visible = !tree.contains(&format!("\"app_id\": \"{}\".*scratchpad", window_class));

        if is_visible {
            // Terminal is visible, move it to scratchpad (hide it)
            let message = format!("[app_id=\"{}\"] move to scratchpad", window_class);
            swaymsg(&message)?;
            println!("Hid scratchpad terminal '{}'", config.name);
        } else {
            println!("Scratchpad terminal '{}' is already hidden", config.name);
        }
    } else {
        println!("Scratchpad terminal '{}' does not exist", config.name);
    }

    Ok(())
}

/// Show scratchpad terminal for Hyprland
fn show_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Check if terminal with specific class exists using direct IPC
    let window_exists = check_window_exists(&CompositorType::Hyprland, &window_class)?;

    if window_exists {
        // Terminal exists, show special workspace using direct IPC
        hyprland_ipc::show_special_workspace(&workspace_name)?;
        println!("Showed scratchpad terminal '{}'", config.name);
    } else {
        // Terminal doesn't exist, create it with proper rules
        create_and_configure_hyprland_scratchpad(config)?;

        // Show the special workspace using direct IPC
        hyprland_ipc::show_special_workspace(&workspace_name)?;

        println!("Scratchpad terminal '{}' created and shown", config.name);
    }

    Ok(())
}

/// Hide scratchpad terminal for Hyprland
fn hide_scratchpad_hyprland(config: &ScratchpadConfig) -> Result<()> {
    let workspace_name = config.workspace_name();
    let window_class = config.window_class();

    // Check if terminal with specific class exists using direct IPC
    let window_exists = check_window_exists(&CompositorType::Hyprland, &window_class)?;

    if window_exists {
        // Check if special workspace is currently active
        let is_visible =
            hyprland_ipc::is_special_workspace_active(&workspace_name).unwrap_or(false);

        if is_visible {
            // Terminal is visible, hide special workspace using direct IPC
            hyprland_ipc::hide_special_workspace(&workspace_name)?;
            println!("Hid scratchpad terminal '{}'", config.name);
        } else {
            println!("Scratchpad terminal '{}' is already hidden", config.name);
        }
    } else {
        println!("Scratchpad terminal '{}' does not exist", config.name);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ScratchpadConfig::default();
        assert_eq!(config.name, "instantscratchpad");
        assert_eq!(config.window_class(), "scratchpad_instantscratchpad");
        assert_eq!(config.workspace_name(), "scratchpad_instantscratchpad");
        assert_eq!(config.terminal_command, "kitty");
        assert_eq!(config.inner_command, None);
        assert_eq!(config.width_pct, 50);
        assert_eq!(config.height_pct, 60);
    }

    #[test]
    fn test_custom_config() {
        let config = ScratchpadConfig::with_params(
            "my_scratch".to_string(),
            "alacritty".to_string(),
            Some("fish".to_string()),
            70,
            80,
        );

        assert_eq!(config.name, "my_scratch");
        assert_eq!(config.window_class(), "scratchpad_my_scratch");
        assert_eq!(config.workspace_name(), "scratchpad_my_scratch");
        assert_eq!(config.terminal_command, "alacritty");
        assert_eq!(config.inner_command, Some("fish".to_string()));
        assert_eq!(config.width_pct, 70);
        assert_eq!(config.height_pct, 80);
    }

    #[test]
    fn test_get_terminal_command_with_class() {
        // Test without inner command
        let config = ScratchpadConfig::with_params(
            "test".to_string(),
            "alacritty".to_string(),
            None,
            50,
            60,
        );

        let cmd = get_terminal_command_with_class(&config);
        assert_eq!(cmd, "alacritty --class scratchpad_test");

        // Test with inner command
        let config_with_cmd = ScratchpadConfig::with_params(
            "test".to_string(),
            "kitty".to_string(),
            Some("fish".to_string()),
            50,
            60,
        );

        let cmd_with_inner = get_terminal_command_with_class(&config_with_cmd);
        assert_eq!(cmd_with_inner, "kitty --class scratchpad_test -e fish");

        // Test unsupported terminal
        let config_other =
            ScratchpadConfig::with_params("test".to_string(), "wezterm".to_string(), None, 50, 60);

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

    #[test]
    fn test_wait_for_window_polling() {
        use crate::compositor::CompositorType;

        // Test with unsupported compositor (should return false after max attempts since window doesn't exist)
        let result = wait_for_window_to_appear(
            &CompositorType::Other("test".to_string()),
            "test_window",
            2,  // max attempts
            10, // poll interval (short for test)
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false); // Should return false for unsupported compositor (window doesn't exist)
    }

    #[test]
    fn test_check_window_exists() {
        use crate::compositor::CompositorType;

        // Test with unsupported compositor
        let result = check_window_exists(&CompositorType::Other("test".to_string()), "test_window");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false); // Should return false for unsupported compositor
    }
}

/// Get terminal command with appropriate class flag and inner command
fn get_terminal_command_with_class(config: &ScratchpadConfig) -> String {
    let window_class = config.window_class();
    let base_cmd = match config.terminal_command.as_str() {
        "alacritty" => format!("{} --class {}", config.terminal_command, window_class),
        "kitty" => format!("{} --class {}", config.terminal_command, window_class),
        _ => config.terminal_command.clone(),
    };

    // Add inner command if specified
    if let Some(ref inner_cmd) = config.inner_command {
        match config.terminal_command.as_str() {
            "alacritty" => format!("{} -e {}", base_cmd, inner_cmd),
            "kitty" => format!("{} -e {}", base_cmd, inner_cmd),
            "wezterm" => format!("{} -e {}", base_cmd, inner_cmd),
            _ => format!("{} -e {}", base_cmd, inner_cmd),
        }
    } else {
        base_cmd
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
        ScratchpadCommands::Toggle { args } => {
            if debug {
                eprintln!("Toggle scratchpad with config:");
                eprintln!("  name: {}", args.name);
                eprintln!("  terminal: {}", args.terminal);
                eprintln!("  command: {:?}", args.command);
                eprintln!("  width_pct: {}", args.width_pct);
                eprintln!("  height_pct: {}", args.height_pct);
            }

            let config = ScratchpadConfig::with_params(
                args.name,
                args.terminal,
                args.command,
                args.width_pct,
                args.height_pct,
            );

            match toggle_scratchpad(&compositor, &config) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("Error toggling scratchpad: {}", e);
                    Ok(1)
                }
            }
        }
        ScratchpadCommands::Show { args } => {
            if debug {
                eprintln!("Show scratchpad with config:");
                eprintln!("  name: {}", args.name);
                eprintln!("  terminal: {}", args.terminal);
                eprintln!("  command: {:?}", args.command);
                eprintln!("  width_pct: {}", args.width_pct);
                eprintln!("  height_pct: {}", args.height_pct);
            }

            let config = ScratchpadConfig::with_params(
                args.name,
                args.terminal,
                args.command,
                args.width_pct,
                args.height_pct,
            );

            match show_scratchpad(&compositor, &config) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("Error showing scratchpad: {}", e);
                    Ok(1)
                }
            }
        }
        ScratchpadCommands::Hide { args } => {
            if debug {
                eprintln!("Hide scratchpad for: {}", args.name);
            }

            let config = ScratchpadConfig::new(args.name);

            match hide_scratchpad(&compositor, &config) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("Error hiding scratchpad: {}", e);
                    Ok(1)
                }
            }
        }
        ScratchpadCommands::Status { args } => {
            if debug {
                eprintln!("Check scratchpad status for: {}", args.name);
            }

            let config = ScratchpadConfig::new(args.name);

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

/// Shared arguments for scratchpad commands that create/configure terminals
#[derive(clap::Args, Debug, Clone)]
pub struct ScratchpadCreateArgs {
    /// Scratchpad name (used as prefix for window class)
    #[arg(long, default_value = "instantscratchpad")]
    pub name: String,
    /// Terminal command to launch
    #[arg(long, default_value = "kitty")]
    pub terminal: String,
    /// Command to run inside the terminal (e.g., "fish", "ranger", "yazi")
    #[arg(long)]
    pub command: Option<String>,
    /// Terminal width as percentage of screen
    #[arg(long, default_value = "50")]
    pub width_pct: u32,
    /// Terminal height as percentage of screen
    #[arg(long, default_value = "60")]
    pub height_pct: u32,
}

/// Shared arguments for scratchpad commands that only need identification
#[derive(clap::Args, Debug, Clone)]
pub struct ScratchpadIdentifyArgs {
    /// Scratchpad name (used as prefix for window class)
    #[arg(long, default_value = "instantscratchpad")]
    pub name: String,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum ScratchpadCommands {
    /// Toggle scratchpad terminal visibility
    Toggle {
        #[command(flatten)]
        args: ScratchpadCreateArgs,
    },
    /// Show scratchpad terminal (create if it doesn't exist)
    Show {
        #[command(flatten)]
        args: ScratchpadCreateArgs,
    },
    /// Hide scratchpad terminal
    Hide {
        #[command(flatten)]
        args: ScratchpadIdentifyArgs,
    },
    /// Check if scratchpad terminal is currently visible
    Status {
        #[command(flatten)]
        args: ScratchpadIdentifyArgs,
    },
}
