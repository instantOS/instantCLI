//! InstantWM control utilities
//!
//! Provides a type-safe interface for controlling instantWM settings.
//! Uses `instantwmctl` to communicate with instantWM over its IPC socket.
//!
//! For scratchpad operations, see `crate::common::compositor::instantwm`.

use anyhow::Result;

use crate::common::instantwmctl;

/// Represents controllable settings in instantWM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstantWmSetting {
    Animated,
}

/// Explicit toggle actions supported by instantWM's IPC interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    Disable,
    Enable,
}

impl InstantWmSetting {
    fn command_name(&self) -> &'static str {
        match self {
            InstantWmSetting::Animated => "animated",
        }
    }
}

impl ControlAction {
    fn arg(&self) -> &'static str {
        match self {
            ControlAction::Disable => "off",
            ControlAction::Enable => "on",
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
        instantwmctl::run(["toggle", setting.command_name(), action.arg()])
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
        assert_eq!(ControlAction::Disable.arg(), "off");
        assert_eq!(ControlAction::Enable.arg(), "on");
    }

    #[test]
    fn test_setting_control_ids() {
        assert_eq!(InstantWmSetting::Animated.command_name(), "animated");
    }
}
