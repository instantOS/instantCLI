//! Display management types and utilities
//!
//! Shared types for display/monitor configuration across settings and doctor checks.

mod sway;

pub use sway::SwayDisplayProvider;

/// Represents a display mode with resolution and refresh rate
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    /// Refresh rate in milliHz (e.g., 60008 = 60.008 Hz)
    pub refresh: u32,
}

impl DisplayMode {
    /// Resolution as total pixels
    pub fn resolution(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Refresh rate in Hz for display
    pub fn refresh_hz(&self) -> f64 {
        self.refresh as f64 / 1000.0
    }

    /// Format for swaymsg command (e.g., "1920x1080@60.008Hz")
    pub fn to_swaymsg_format(&self) -> String {
        format!("{}x{}@{:.3}Hz", self.width, self.height, self.refresh_hz())
    }

    /// Human-readable display format (e.g., "1920x1080 @ 60Hz")
    pub fn display_format(&self) -> String {
        format!("{}x{} @ {:.0}Hz", self.width, self.height, self.refresh_hz())
    }
}

/// Information about a display output
#[derive(Debug, Clone)]
pub struct OutputInfo {
    /// Display name (e.g., "eDP-1", "HDMI-A-1")
    pub name: String,
    /// Display make/manufacturer
    pub make: String,
    /// Display model
    pub model: String,
    /// Currently active mode
    pub current_mode: DisplayMode,
    /// All available modes for this output
    pub available_modes: Vec<DisplayMode>,
}

impl OutputInfo {
    /// Get the optimal (highest resolution, then highest refresh) mode
    pub fn optimal_mode(&self) -> DisplayMode {
        self.available_modes
            .iter()
            .max_by(|a, b| {
                a.resolution()
                    .cmp(&b.resolution())
                    .then(a.refresh.cmp(&b.refresh))
            })
            .cloned()
            .unwrap_or_else(|| self.current_mode.clone())
    }

    /// Check if current mode is optimal
    pub fn is_optimal(&self) -> bool {
        self.current_mode == self.optimal_mode()
    }

    /// Get a human-readable display label
    pub fn display_label(&self) -> String {
        let model_info = if !self.model.is_empty() && self.model != "Unknown" {
            format!(" ({})", self.model)
        } else if !self.make.is_empty() && self.make != "Unknown" {
            format!(" ({})", self.make)
        } else {
            String::new()
        };
        format!(
            "{}{}: {}",
            self.name,
            model_info,
            self.current_mode.display_format()
        )
    }
}
