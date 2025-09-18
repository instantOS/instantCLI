use serde::{Deserialize, Serialize};
use std::env;
use std::process::Command;

pub mod hyprland;
pub mod sway;

/// Window compositor types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompositorType {
    /// Sway compositor (i3-compatible Wayland compositor)
    Sway,
    /// Hyprland compositor (dynamic tiling Wayland compositor)
    Hyprland,
    /// Other/unknown compositor
    Other(String),
}

impl CompositorType {
    /// Detect the current window compositor
    pub fn detect() -> Self {
        // Check environment variables first
        if let Ok(session) = env::var("XDG_SESSION_DESKTOP") {
            match session.to_lowercase().as_str() {
                "sway" => return CompositorType::Sway,
                "hyprland" => return CompositorType::Hyprland,
                _ => {}
            }
        }

        if let Ok(desktop) = env::var("DESKTOP_SESSION") {
            match desktop.to_lowercase().as_str() {
                "sway" => return CompositorType::Sway,
                "hyprland" => return CompositorType::Hyprland,
                _ => {}
            }
        }

        // Check for Wayland display server
        if env::var("WAYLAND_DISPLAY").is_ok() {
            // Try to detect specific Wayland compositors
            if CompositorType::is_process_running("sway") {
                return CompositorType::Sway;
            }
            if CompositorType::is_process_running("Hyprland") {
                return CompositorType::Hyprland;
            }
        }

        // Check for X11 display server
        if env::var("DISPLAY").is_ok() {
            // Could check for X11 window managers here if needed
            return CompositorType::Other("x11".to_string());
        }

        // Fallback
        CompositorType::Other("unknown".to_string())
    }

    /// Check if a process with the given name is running
    fn is_process_running(process_name: &str) -> bool {
        // Try pgrep first (most reliable)
        if let Ok(output) = Command::new("pgrep").arg(process_name).output() {
            if !output.stdout.is_empty() {
                return true;
            }
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

    /// Get a human-readable name for the compositor
    pub fn name(&self) -> String {
        match self {
            CompositorType::Sway => "Sway".to_string(),
            CompositorType::Hyprland => "Hyprland".to_string(),
            CompositorType::Other(name) => name.clone(),
        }
    }

    /// Check if the compositor is Wayland-based
    pub fn is_wayland(&self) -> bool {
        match self {
            CompositorType::Sway | CompositorType::Hyprland => true,
            CompositorType::Other(name) => name.to_lowercase().contains("wayland"),
        }
    }

    /// Check if the compositor is X11-based
    pub fn is_x11(&self) -> bool {
        match self {
            CompositorType::Other(name) => name.to_lowercase().contains("x11"),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compositor_detection() {
        let compositor = CompositorType::detect();
        // We can't test the exact result since it depends on the environment
        // but we can test that it returns a valid variant
        match compositor {
            CompositorType::Sway | CompositorType::Hyprland | CompositorType::Other(_) => {
                // Test passes
            }
        }
    }

    #[test]
    fn test_compositor_name() {
        assert_eq!(CompositorType::Sway.name(), "Sway");
        assert_eq!(CompositorType::Hyprland.name(), "Hyprland");
        assert_eq!(CompositorType::Other("test".to_string()).name(), "test");
    }

    #[test]
    fn test_wayland_detection() {
        assert!(CompositorType::Sway.is_wayland());
        assert!(CompositorType::Hyprland.is_wayland());
        assert!(!CompositorType::Other("x11".to_string()).is_wayland());
        assert!(CompositorType::Other("wayland-other".to_string()).is_wayland());
    }

    #[test]
    fn test_x11_detection() {
        assert!(!CompositorType::Sway.is_x11());
        assert!(!CompositorType::Hyprland.is_x11());
        assert!(CompositorType::Other("x11".to_string()).is_x11());
        assert!(!CompositorType::Other("wayland".to_string()).is_x11());
    }
}
