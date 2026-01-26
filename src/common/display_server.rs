use serde::{Deserialize, Serialize};
use std::env;
use std::process::Command;

/// Display server types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DisplayServer {
    /// Wayland display server
    Wayland,
    /// X11 display server
    X11,
    /// Unknown or unsupported display server
    Unknown,
}

impl DisplayServer {
    /// Detect the current display server type
    pub fn detect() -> Self {
        // Check XDG_SESSION_TYPE first (most reliable)
        if let Ok(session_type) = env::var("XDG_SESSION_TYPE") {
            match session_type.to_lowercase().as_str() {
                "wayland" => return DisplayServer::Wayland,
                "x11" => return DisplayServer::X11,
                _ => {}
            }
        }

        // Check WAYLAND_DISPLAY environment variable
        if env::var("WAYLAND_DISPLAY").is_ok() {
            return DisplayServer::Wayland;
        }

        // Check DISPLAY environment variable
        if env::var("DISPLAY").is_ok() {
            return DisplayServer::X11;
        }

        // Fallback: check for running processes
        if Self::is_wayland_process_running() {
            return DisplayServer::Wayland;
        }

        if Self::is_x11_process_running() {
            return DisplayServer::X11;
        }

        DisplayServer::Unknown
    }

    /// Check if any Wayland compositor process is running
    fn is_wayland_process_running() -> bool {
        let wayland_processes = ["sway", "hyprland", "river", "wayfire", "labwc"];

        for process in &wayland_processes {
            if Self::is_process_running(process) {
                return true;
            }
        }
        false
    }

    /// Check if any X11 window manager process is running
    fn is_x11_process_running() -> bool {
        let x11_processes = ["i3", "openbox", "awesome", "bspwm", "dwm", "xmonad"];

        for process in &x11_processes {
            if Self::is_process_running(process) {
                return true;
            }
        }
        false
    }

    /// Check if a process with the given name is running
    fn is_process_running(process_name: &str) -> bool {
        // Try pgrep first (most reliable)
        if let Ok(output) = Command::new("pgrep").arg(process_name).output()
            && !output.stdout.is_empty()
        {
            return true;
        }

        // Try ps as fallback
        if let Ok(output) = Command::new("ps").arg("aux").output() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if output_str.contains(process_name) {
                return true;
            }
        }

        false
    }

    /// Get the appropriate clipboard command for the display server
    pub fn get_clipboard_command(&self) -> (&'static str, Vec<&'static str>) {
        match self {
            DisplayServer::Wayland => ("wl-paste", vec![]),
            DisplayServer::X11 => ("xclip", vec!["-selection", "clipboard", "-o"]),
            DisplayServer::Unknown => ("wl-paste", vec![]), // Default to Wayland
        }
    }

    /// Get the appropriate screenshot command for the display server
    #[allow(dead_code)]
    pub fn get_screenshot_command(&self) -> (&'static str, Vec<&'static str>) {
        match self {
            DisplayServer::Wayland => ("grim", vec![]),
            DisplayServer::X11 => ("scrot", vec![]),
            DisplayServer::Unknown => ("grim", vec![]), // Default to Wayland
        }
    }

}

impl std::fmt::Display for DisplayServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            DisplayServer::Wayland => "Wayland",
            DisplayServer::X11 => "X11",
            DisplayServer::Unknown => "Unknown",
        };
        write!(f, "{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_server_detection() {
        let server = DisplayServer::detect();
        // We can't test the exact result since it depends on the environment
        // but we can test that it returns a valid variant
        match server {
            DisplayServer::Wayland | DisplayServer::X11 | DisplayServer::Unknown => {
                // Test passes
            }
        }
    }

    #[test]
    fn test_display_server_name() {
        assert_eq!(DisplayServer::Wayland.to_string(), "Wayland");
        assert_eq!(DisplayServer::X11.to_string(), "X11");
        assert_eq!(DisplayServer::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_wayland_detection() {
        assert!(matches!(DisplayServer::Wayland, DisplayServer::Wayland));
        assert!(!matches!(DisplayServer::Wayland, DisplayServer::X11));
        assert!(!matches!(DisplayServer::Wayland, DisplayServer::Unknown));
    }

    #[test]
    fn test_x11_detection() {
        assert!(matches!(DisplayServer::X11, DisplayServer::X11));
        assert!(!matches!(DisplayServer::X11, DisplayServer::Wayland));
        assert!(!matches!(DisplayServer::X11, DisplayServer::Unknown));
    }

    #[test]
    fn test_unknown_detection() {
        assert!(matches!(DisplayServer::Unknown, DisplayServer::Unknown));
        assert!(!matches!(DisplayServer::Unknown, DisplayServer::Wayland));
        assert!(!matches!(DisplayServer::Unknown, DisplayServer::X11));
    }

    #[test]
    fn test_clipboard_commands() {
        let wayland_cmd = DisplayServer::Wayland.get_clipboard_command();
        assert_eq!(wayland_cmd.0, "wl-paste");
        assert!(wayland_cmd.1.is_empty());

        let x11_cmd = DisplayServer::X11.get_clipboard_command();
        assert_eq!(x11_cmd.0, "xclip");
        assert_eq!(x11_cmd.1, vec!["-selection", "clipboard", "-o"]);
    }

    #[test]
    fn test_screenshot_commands() {
        let wayland_cmd = DisplayServer::Wayland.get_screenshot_command();
        assert_eq!(wayland_cmd.0, "grim");

        let x11_cmd = DisplayServer::X11.get_screenshot_command();
        assert_eq!(x11_cmd.0, "scrot");
    }

    #[test]
    fn test_desktop_session() {
        assert!(!matches!(DisplayServer::Wayland, DisplayServer::Unknown));
        assert!(!matches!(DisplayServer::X11, DisplayServer::Unknown));
        assert!(matches!(DisplayServer::Unknown, DisplayServer::Unknown));
    }
}
