/// Supported terminal emulators
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Terminal {
    #[default]
    Kitty,
    Alacritty,
    Wezterm,
    Other(String),
}

impl Terminal {
    /// Get the command name for this terminal
    pub fn command(&self) -> &str {
        match self {
            Terminal::Kitty => "kitty",
            Terminal::Alacritty => "alacritty",
            Terminal::Wezterm => "wezterm",
            Terminal::Other(cmd) => cmd,
        }
    }

    /// Get the class flag for this terminal
    pub fn class_flag(&self, class: &str) -> String {
        match self {
            Terminal::Kitty => format!("--class {class}"),
            Terminal::Alacritty => format!("--class {class}"),
            Terminal::Wezterm => format!("--class {class}"),
            Terminal::Other(_) => format!("--class {class}"), // Assume standard flag
        }
    }

    /// Get the execute flag for running commands in this terminal
    pub fn execute_flag(&self) -> &str {
        match self {
            Terminal::Kitty => "-e",
            Terminal::Alacritty => "-e",
            Terminal::Wezterm => "-e",
            Terminal::Other(_) => "-e", // Assume standard -e flag
        }
    }
}

impl From<String> for Terminal {
    fn from(s: String) -> Self {
        match s.as_str() {
            "kitty" => Terminal::Kitty,
            "alacritty" => Terminal::Alacritty,
            "wezterm" => Terminal::Wezterm,
            _ => Terminal::Other(s),
        }
    }
}

impl From<&str> for Terminal {
    fn from(s: &str) -> Self {
        Terminal::from(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_command() {
        assert_eq!(Terminal::Kitty.command(), "kitty");
        assert_eq!(Terminal::Alacritty.command(), "alacritty");
        assert_eq!(Terminal::Wezterm.command(), "wezterm");
        assert_eq!(Terminal::Other("foot".to_string()).command(), "foot");
    }

    #[test]
    fn test_terminal_class_flag() {
        assert_eq!(Terminal::Kitty.class_flag("test"), "--class test");
        assert_eq!(Terminal::Alacritty.class_flag("test"), "--class test");
        assert_eq!(Terminal::Wezterm.class_flag("test"), "--class test");
        assert_eq!(
            Terminal::Other("foot".to_string()).class_flag("test"),
            "--class test"
        );
    }

    #[test]
    fn test_terminal_from_string() {
        assert_eq!(Terminal::from("kitty"), Terminal::Kitty);
        assert_eq!(Terminal::from("alacritty"), Terminal::Alacritty);
        assert_eq!(Terminal::from("wezterm"), Terminal::Wezterm);
        assert_eq!(Terminal::from("foot"), Terminal::Other("foot".to_string()));
    }

    #[test]
    fn test_terminal_default() {
        assert_eq!(Terminal::default(), Terminal::Kitty);
    }
}
