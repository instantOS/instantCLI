use super::display_server::DisplayServer;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::process::Command;

pub mod fallback;
pub mod gnome;
pub mod hyprland;
pub mod i3;
pub mod kwin;
pub mod sway;

/// Create and launch terminal in background
pub fn create_terminal_process(config: &ScratchpadConfig) -> Result<()> {
    let term_cmd = config.terminal_command();
    let bg_cmd = format!("nohup {term_cmd} >/dev/null 2>&1 &");

    Command::new("sh")
        .args(["-c", &bg_cmd])
        .output()
        .context("Failed to launch terminal in background")?;

    Ok(())
}

/// Trait for scratchpad providers
pub trait ScratchpadProvider: Send + Sync {
    /// Show the scratchpad
    fn show(&self, config: &ScratchpadConfig) -> Result<()>;
    /// Hide the scratchpad
    fn hide(&self, config: &ScratchpadConfig) -> Result<()>;
    /// Toggle the scratchpad
    fn toggle(&self, config: &ScratchpadConfig) -> Result<()>;
    /// Get all scratchpad windows
    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>>;
    /// Check if the scratchpad window is running
    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool>;
    /// Check if the scratchpad window is visible
    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool>;
    /// Show the scratchpad without checking if it exists (optimistic)
    fn show_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        self.show(config)
    }
    /// Hide the scratchpad without checking if it exists (optimistic)
    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        self.hide(config)
    }
}

/// Information about a scratchpad window
#[derive(Debug, Clone)]
pub struct ScratchpadWindowInfo {
    pub name: String,
    pub window_class: String,
    pub title: String,
    pub visible: bool,
}

/// Window compositor types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompositorType {
    /// i3-wm compositor (X11 tiling window manager)
    I3,
    /// dwm (dynamic window manager)
    Dwm,
    /// InstantWM (dwm fork)
    InstantWM,
    /// Sway compositor (i3-compatible Wayland compositor)
    Sway,
    /// Hyprland compositor (dynamic tiling Wayland compositor)
    Hyprland,
    /// KDE KWin compositor
    KWin,
    /// Gnome compositor
    Gnome,
    /// Other/unknown compositor
    Other(String),
}

impl CompositorType {
    /// Detect the current window compositor
    pub fn detect() -> Self {
        // Check environment variables first
        if let Ok(session) = env::var("XDG_SESSION_DESKTOP") {
            match session.to_lowercase().as_str() {
                "i3" => return CompositorType::I3,
                "dwm" => return CompositorType::Dwm,
                "instantwm" => return CompositorType::InstantWM,
                "sway" => return CompositorType::Sway,
                "hyprland" => return CompositorType::Hyprland,
                "kde" | "plasma" | "kwin" => return CompositorType::KWin,
                s if s.contains("gnome") => return CompositorType::Gnome,
                _ => {}
            }
        }

        if let Ok(desktop) = env::var("DESKTOP_SESSION") {
            match desktop.to_lowercase().as_str() {
                "i3" => return CompositorType::I3,
                "dwm" => return CompositorType::Dwm,
                "instantwm" => return CompositorType::InstantWM,
                "sway" => return CompositorType::Sway,
                "hyprland" => return CompositorType::Hyprland,
                "kde" | "plasma" | "kwin" => return CompositorType::KWin,
                s if s.contains("gnome") => return CompositorType::Gnome,
                _ => {}
            }
        }

        // Check XDG_CURRENT_DESKTOP for KDE
        if let Ok(current) = env::var("XDG_CURRENT_DESKTOP") {
            if current.to_lowercase() == "kde" {
                return CompositorType::KWin;
            }
        }

        // Use display server detection to guide compositor detection
        match DisplayServer::detect() {
            DisplayServer::Wayland => {
                // Try to detect specific Wayland compositors
                if CompositorType::is_process_running("sway") {
                    return CompositorType::Sway;
                }
                if CompositorType::is_process_running("Hyprland") {
                    return CompositorType::Hyprland;
                }
                if CompositorType::is_process_running("kwin_wayland") {
                    return CompositorType::KWin;
                }
                if CompositorType::is_process_running("gnome-shell") {
                    return CompositorType::Gnome;
                }
                CompositorType::Other("wayland".to_string())
            }
            DisplayServer::X11 => {
                // Check for X11 window managers
                if CompositorType::is_process_running("i3") {
                    return CompositorType::I3;
                }
                if CompositorType::is_process_running("dwm") {
                    return CompositorType::Dwm;
                }
                if CompositorType::is_process_running("instantwm") {
                    return CompositorType::InstantWM;
                }
                if CompositorType::is_process_running("kwin_x11") {
                    return CompositorType::KWin;
                }
                if CompositorType::is_process_running("gnome-shell") {
                    return CompositorType::Gnome;
                }
                CompositorType::Other("x11".to_string())
            }
            DisplayServer::Unknown => CompositorType::Other("unknown".to_string()),
        }
    }

    /// Get the scratchpad provider for this compositor
    pub fn provider(&self) -> Box<dyn ScratchpadProvider> {
        match self {
            CompositorType::I3 => Box::new(i3::I3),
            CompositorType::Dwm => Box::new(fallback::Fallback),
            CompositorType::InstantWM => Box::new(fallback::Fallback), // TODO: Add InstantWM scratchpad support if needed
            CompositorType::Sway => Box::new(sway::Sway),
            CompositorType::Hyprland => Box::new(hyprland::Hyprland),
            CompositorType::KWin => Box::new(kwin::KWin),
            CompositorType::Gnome => Box::new(gnome::Gnome),
            CompositorType::Other(_) => Box::new(fallback::Fallback),
        }
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

    /// Get a human-readable name for the compositor
    pub fn name(&self) -> String {
        match self {
            CompositorType::I3 => "i3".to_string(),
            CompositorType::Dwm => "dwm".to_string(),
            CompositorType::InstantWM => "instantwm".to_string(),
            CompositorType::Sway => "Sway".to_string(),
            CompositorType::Hyprland => "Hyprland".to_string(),
            CompositorType::KWin => "KWin".to_string(),
            CompositorType::Gnome => "Gnome".to_string(),
            CompositorType::Other(name) => name.clone(),
        }
    }

    /// Check if the compositor is Wayland-based
    #[allow(dead_code)]
    pub fn is_wayland(&self) -> bool {
        match self {
            CompositorType::Sway | CompositorType::Hyprland => true,
            CompositorType::KWin => DisplayServer::detect() == DisplayServer::Wayland,
            CompositorType::Gnome => DisplayServer::detect() == DisplayServer::Wayland,
            CompositorType::Other(name) => name.to_lowercase().contains("wayland"),
            _ => false,
        }
    }

    /// Check if the compositor is X11-based
    #[allow(dead_code)]
    pub fn is_x11(&self) -> bool {
        match self {
            CompositorType::I3 => true,
            CompositorType::Dwm => true,
            CompositorType::InstantWM => true,
            CompositorType::KWin => DisplayServer::detect() == DisplayServer::X11,
            CompositorType::Gnome => DisplayServer::detect() == DisplayServer::X11,
            CompositorType::Other(name) => name.to_lowercase().contains("x11"),
            _ => false,
        }
    }

    /// Get the display server type for this compositor
    #[allow(dead_code)]
    pub fn display_server(&self) -> DisplayServer {
        match self {
            CompositorType::Sway | CompositorType::Hyprland => DisplayServer::Wayland,
            CompositorType::I3 => DisplayServer::X11,
            CompositorType::Dwm => DisplayServer::X11,
            CompositorType::InstantWM => DisplayServer::X11,
            CompositorType::KWin => DisplayServer::detect(),
            CompositorType::Gnome => DisplayServer::detect(),
            CompositorType::Other(name) => {
                if name.to_lowercase().contains("wayland") {
                    DisplayServer::Wayland
                } else if name.to_lowercase().contains("x11") {
                    DisplayServer::X11
                } else {
                    DisplayServer::Unknown
                }
            }
        }
    }

    /// Get all scratchpad windows for this compositor
    pub fn get_all_scratchpad_windows(&self) -> anyhow::Result<Vec<ScratchpadWindowInfo>> {
        self.provider().get_all_windows()
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
            CompositorType::I3
            | CompositorType::Sway
            | CompositorType::Hyprland
            | CompositorType::KWin
            | CompositorType::Gnome
            | CompositorType::Dwm
            | CompositorType::InstantWM
            | CompositorType::Other(_) => {
                // Test passes
            }
        }
    }

    #[test]
    fn test_compositor_name() {
        assert_eq!(CompositorType::Sway.name(), "Sway");
        assert_eq!(CompositorType::Hyprland.name(), "Hyprland");
        assert_eq!(CompositorType::KWin.name(), "KWin");
        assert_eq!(CompositorType::Gnome.name(), "Gnome");
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
