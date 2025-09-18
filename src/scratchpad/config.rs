use super::terminal::Terminal;

/// Configuration for scratchpad terminal behavior
#[derive(Debug, Clone)]
pub struct ScratchpadConfig {
    /// Scratchpad name (used as prefix for window class)
    pub name: String,
    /// Terminal emulator to use
    pub terminal: Terminal,
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
            terminal: Terminal::default(),
            inner_command: None,
            width_pct: 50,
            height_pct: 60,
        }
    }

    /// Create config with custom parameters
    pub fn with_params(
        name: String,
        terminal: Terminal,
        inner_command: Option<String>,
        width_pct: u32,
        height_pct: u32,
    ) -> Self {
        Self {
            name,
            terminal,
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

    /// Generate the terminal command with appropriate class flag and inner command
    pub fn terminal_command(&self) -> String {
        let window_class = self.window_class();
        let base_cmd = format!("{} {}", self.terminal.command(), self.terminal.class_flag(&window_class));

        // Add inner command if specified
        if let Some(ref inner_cmd) = self.inner_command {
            format!("{} {} {}", base_cmd, self.terminal.execute_flag(), inner_cmd)
        } else {
            base_cmd
        }
    }
}

impl Default for ScratchpadConfig {
    fn default() -> Self {
        Self::new("instantscratchpad".to_string())
    }
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
        assert_eq!(config.terminal, Terminal::Kitty);
        assert_eq!(config.inner_command, None);
        assert_eq!(config.width_pct, 50);
        assert_eq!(config.height_pct, 60);
    }

    #[test]
    fn test_custom_config() {
        let config = ScratchpadConfig::with_params(
            "my_scratch".to_string(),
            Terminal::Alacritty,
            Some("fish".to_string()),
            70,
            80,
        );
        assert_eq!(config.name, "my_scratch");
        assert_eq!(config.window_class(), "scratchpad_my_scratch");
        assert_eq!(config.workspace_name(), "scratchpad_my_scratch");
        assert_eq!(config.terminal, Terminal::Alacritty);
        assert_eq!(config.inner_command, Some("fish".to_string()));
        assert_eq!(config.width_pct, 70);
        assert_eq!(config.height_pct, 80);
    }

    #[test]
    fn test_terminal_command_generation() {
        // Test without inner command
        let config = ScratchpadConfig::with_params(
            "test".to_string(),
            Terminal::Alacritty,
            None,
            50,
            60,
        );

        let cmd = config.terminal_command();
        assert_eq!(cmd, "alacritty --class scratchpad_test");

        // Test with inner command
        let config_with_cmd = ScratchpadConfig::with_params(
            "test".to_string(),
            Terminal::Kitty,
            Some("fish".to_string()),
            50,
            60,
        );

        let cmd_with_inner = config_with_cmd.terminal_command();
        assert_eq!(cmd_with_inner, "kitty --class scratchpad_test -e fish");

        // Test wezterm terminal
        let config_wezterm =
            ScratchpadConfig::with_params("test".to_string(), Terminal::Wezterm, None, 50, 60);

        let cmd_wezterm = config_wezterm.terminal_command();
        assert_eq!(cmd_wezterm, "wezterm --class scratchpad_test");

        // Test other terminal
        let config_other = ScratchpadConfig::with_params(
            "test".to_string(),
            Terminal::Other("foot".to_string()),
            None,
            50,
            60,
        );

        let cmd_other = config_other.terminal_command();
        assert_eq!(cmd_other, "foot --class scratchpad_test");
    }
}
