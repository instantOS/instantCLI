//! InstantWM control utilities
//!
//! Provides a type-safe interface for controlling instantWM settings.
//! Uses `xsetroot -name` to communicate with instantWM via the underlying
//! `ctrltoggle` mechanism.
//!
//! For scratchpad operations, see `crate::common::compositor::instantwm`.

use anyhow::{Context, Result};
use std::process::Command;

/// Represents controllable settings in instantWM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstantWmSetting {
    Animated,
}

/// Control toggle arguments for instantWM's ctrltoggle function
///
/// These map to instantWM's internal ctrltoggle behavior:
/// - 1: set to 0 (disable)
/// - 3: set to 1 (enable)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    Disable,
    Enable,
}

impl InstantWmSetting {
    fn control_id(&self) -> &'static str {
        match self {
            InstantWmSetting::Animated => "animated",
        }
    }
}

impl ControlAction {
    fn arg(&self) -> u8 {
        match self {
            ControlAction::Disable => 1,
            ControlAction::Enable => 3,
        }
    }
}

/// Controller for instantWM window manager settings
pub struct InstantWmController;

impl InstantWmController {
    pub fn new() -> Self {
        Self
    }

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

    pub fn enable_animations(&self) -> Result<()> {
        self.apply(InstantWmSetting::Animated, ControlAction::Enable)
    }

    pub fn disable_animations(&self) -> Result<()> {
        self.apply(InstantWmSetting::Animated, ControlAction::Disable)
    }

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
        assert_eq!(ControlAction::Disable.arg(), 1);
        assert_eq!(ControlAction::Enable.arg(), 3);
    }

    #[test]
    fn test_setting_control_ids() {
        assert_eq!(InstantWmSetting::Animated.control_id(), "animated");
    }
}
