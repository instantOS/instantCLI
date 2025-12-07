//! InstantWM control utilities
//!
//! Provides a type-safe interface for controlling instantWM window manager.
//! Uses `xsetroot -name` to communicate with instantWM via the underlying
//! `ctrltoggle` mechanism, bypassing the shell scripts that have limitations.

use anyhow::{Context, Result};
use std::process::Command;

/// Represents controllable settings in instantWM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstantWmSetting {
    /// Window animations (resize, move effects)
    Animated,
    // Future settings can be added here, e.g.:
    // Gaps,
    // Borders,
    // StatusBar,
}

impl InstantWmSetting {
    /// Get the control string identifier used in xsetroot commands
    fn control_id(&self) -> &'static str {
        match self {
            InstantWmSetting::Animated => "animated",
        }
    }
}

/// Control toggle arguments for instantWM's ctrltoggle function
///
/// These map to instantWM's internal ctrltoggle behavior:
/// - 0 or 2: toggle current state
/// - 1: set to 0 (disable)  
/// - 3: set to 1 (enable)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    /// Toggle the current state
    Toggle,
    /// Disable the setting (set to 0)
    Disable,
    /// Enable the setting (set to 1)
    Enable,
}

impl ControlAction {
    /// Get the numeric argument for ctrltoggle
    fn arg(&self) -> u8 {
        match self {
            ControlAction::Toggle => 2,
            ControlAction::Disable => 1,
            ControlAction::Enable => 3,
        }
    }
}

/// Controller for instantWM window manager
///
/// Provides a type-safe interface for controlling instantWM settings.
///
/// # Example
/// ```no_run
/// use crate::common::instantwm::{InstantWmController, InstantWmSetting, ControlAction};
///
/// let controller = InstantWmController::new();
/// controller.apply(InstantWmSetting::Animated, ControlAction::Enable)?;
/// ```
pub struct InstantWmController;

impl InstantWmController {
    /// Create a new InstantWM controller
    pub fn new() -> Self {
        Self
    }

    /// Apply a control action to an instantWM setting
    ///
    /// This sends a command to instantWM via xsetroot to control the specified setting.
    /// The action is idempotent - applying Enable twice will keep the setting enabled.
    pub fn apply(&self, setting: InstantWmSetting, action: ControlAction) -> Result<()> {
        let control_string = format!("c;:;{};{}", setting.control_id(), action.arg());

        let status = Command::new("xsetroot")
            .args(["-name", &control_string])
            .status()
            .context("Failed to execute xsetroot")?;

        if status.success() {
            Ok(())
        } else {
            anyhow::bail!(
                "xsetroot command failed with exit code {}",
                status.code().unwrap_or(-1)
            )
        }
    }

    /// Enable animations in instantWM
    pub fn enable_animations(&self) -> Result<()> {
        self.apply(InstantWmSetting::Animated, ControlAction::Enable)
    }

    /// Disable animations in instantWM
    pub fn disable_animations(&self) -> Result<()> {
        self.apply(InstantWmSetting::Animated, ControlAction::Disable)
    }

    /// Toggle animations in instantWM
    pub fn toggle_animations(&self) -> Result<()> {
        self.apply(InstantWmSetting::Animated, ControlAction::Toggle)
    }

    /// Set animation state based on a boolean
    pub fn set_animations(&self, enabled: bool) -> Result<()> {
        if enabled {
            self.enable_animations()
        } else {
            self.disable_animations()
        }
    }
}

impl Default for InstantWmController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_action_args() {
        assert_eq!(ControlAction::Toggle.arg(), 2);
        assert_eq!(ControlAction::Disable.arg(), 1);
        assert_eq!(ControlAction::Enable.arg(), 3);
    }

    #[test]
    fn test_setting_control_ids() {
        assert_eq!(InstantWmSetting::Animated.control_id(), "animated");
    }
}
