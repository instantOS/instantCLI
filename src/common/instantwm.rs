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

/// Represents scratchpad commands in instantWM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstantWmScratchpadCommand {
    /// Show the scratchpad
    Show,
    /// Hide the scratchpad
    Hide,
    /// Toggle scratchpad visibility
    Toggle,
    /// Get scratchpad status
    Status,
}

impl InstantWmSetting {
    /// Get the control string identifier used in xsetroot commands
    fn control_id(&self) -> &'static str {
        match self {
            InstantWmSetting::Animated => "animated",
        }
    }
}

impl InstantWmScratchpadCommand {
    /// Get the command string used in xsetroot commands
    fn command_id(&self) -> &'static str {
        match self {
            InstantWmScratchpadCommand::Show => "showscratchpad",
            InstantWmScratchpadCommand::Hide => "hidescratchpad",
            InstantWmScratchpadCommand::Toggle => "togglescratchpad",
            InstantWmScratchpadCommand::Status => "scratchpadstatus",
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

    /// Apply a scratchpad command to instantWM
    ///
    /// This sends a scratchpad command to instantWM via xsetroot.
    pub fn apply_scratchpad(&self, command: InstantWmScratchpadCommand) -> Result<()> {
        let control_string = format!("c;:;{}", command.command_id());

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

    /// Show the scratchpad
    pub fn show_scratchpad(&self) -> Result<()> {
        self.apply_scratchpad(InstantWmScratchpadCommand::Show)
    }

    /// Hide the scratchpad
    pub fn hide_scratchpad(&self) -> Result<()> {
        self.apply_scratchpad(InstantWmScratchpadCommand::Hide)
    }

    /// Toggle scratchpad visibility
    pub fn toggle_scratchpad(&self) -> Result<()> {
        self.apply_scratchpad(InstantWmScratchpadCommand::Toggle)
    }

    /// Get scratchpad status
    pub fn get_scratchpad_status(&self) -> Result<bool> {
        self.apply_scratchpad(InstantWmScratchpadCommand::Status)?;

        // Wait for the response in WM_NAME
        for attempt in 0..20 {
            if let Ok(output) = Command::new("xprop")
                .args(["-root", "-notype", "WM_NAME"])
                .output()
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(captures) = stdout.strip_prefix("WM_NAME = ") {
                        let value = captures.trim().trim_matches('"');
                        if let Some(status_str) = value.strip_prefix("ipc:scratchpad:") {
                            return Ok(status_str == "1");
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // If we couldn't get the status, assume it's hidden
        Ok(false)
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

    #[test]
    fn test_scratchpad_command_ids() {
        assert_eq!(
            InstantWmScratchpadCommand::Show.command_id(),
            "showscratchpad"
        );
        assert_eq!(
            InstantWmScratchpadCommand::Hide.command_id(),
            "hidescratchpad"
        );
        assert_eq!(
            InstantWmScratchpadCommand::Toggle.command_id(),
            "togglescratchpad"
        );
        assert_eq!(
            InstantWmScratchpadCommand::Status.command_id(),
            "scratchpadstatus"
        );
    }
}
