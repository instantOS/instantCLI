//! InstantWM control utilities
//!
//! Typed wrappers over instantWM's `instantwmctl` surface. Process plumbing
//! lives in [`crate::common::instantwmctl`]; scratchpad operations live in
//! [`crate::common::compositor::instantwm`].
//!
//! Input settings are addressed by their `[input]` entry identifier.
//! instantWM resolves a device against the *first* entry that has the field
//! set: the device identifier, then `type:<kind>`, then the `*` wildcard — and
//! it picks one whole entry, not one field. A `*` write is therefore dead the
//! moment a `type:` entry exists (instantCLI itself creates those for natural
//! scrolling and button swapping), so the helpers below always take the entry
//! they mean to write.

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

use crate::common::instantwmctl;

/// `[input]` entry for devices that have a pointer (mice, trackballs).
pub const POINTER: &str = "type:pointer";
/// `[input]` entry for gesture-capable touchpads.
pub const TOUCHPAD: &str = "type:touchpad";
/// `[input]` wildcard entry, instantWM's last-resort fallback.
pub const WILDCARD: &str = "*";

/// A runtime option instantWM exposes as a boolean `config` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstantWmSetting {
    Animated,
}

/// Explicit on/off state for a [`InstantWmSetting`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    Disable,
    Enable,
}

impl InstantWmSetting {
    /// Key accepted by `instantwmctl config get|set|toggle`.
    fn config_key(&self) -> &'static str {
        match self {
            InstantWmSetting::Animated => "animations.enabled",
        }
    }
}

impl ControlAction {
    /// `config set` takes the value itself, not a verb.
    fn value(&self) -> &'static str {
        match self {
            ControlAction::Disable => "false",
            ControlAction::Enable => "true",
        }
    }
}

/// Controller for instantWM window manager settings
pub struct InstantWmController;

impl InstantWmController {
    pub fn new() -> Self {
        Self
    }

    /// Set `setting` to an explicit state.
    ///
    /// `config set` is idempotent, unlike `config toggle`, which would flip a
    /// value that is already in the requested state.
    pub fn apply(&self, setting: InstantWmSetting, action: ControlAction) -> Result<()> {
        instantwmctl::run(["config", "set", setting.config_key(), action.value()])
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

/// `ToggleSetting` as `instantwmctl mouse …` spells it.
fn toggle(state: bool) -> &'static str {
    if state { "enabled" } else { "disabled" }
}

/// Enable or disable tap-to-click for one `[input]` entry.
pub fn set_tap(identifier: &str, enabled: bool) -> Result<()> {
    instantwmctl::run(["mouse", "tap", toggle(enabled), "--identifier", identifier])
}

/// Enable or disable natural scrolling for one `[input]` entry.
pub fn set_natural_scroll(identifier: &str, enabled: bool) -> Result<()> {
    instantwmctl::run([
        "mouse",
        "natural-scroll",
        toggle(enabled),
        "--identifier",
        identifier,
    ])
}

/// Enable or disable left-handed (swapped) buttons for one `[input]` entry.
pub fn set_left_handed(identifier: &str, enabled: bool) -> Result<()> {
    instantwmctl::run([
        "mouse",
        "left-handed",
        toggle(enabled),
        "--identifier",
        identifier,
    ])
}

/// Set the acceleration profile (`flat`/`adaptive`) for one `[input]` entry.
pub fn set_accel_profile(identifier: &str, profile: &str) -> Result<()> {
    instantwmctl::run([
        "mouse",
        "accel-profile",
        profile,
        "--identifier",
        identifier,
    ])
}

/// Set pointer acceleration speed (`-1.0`..`1.0`) for one `[input]` entry.
pub fn set_pointer_accel(identifier: &str, value: f64) -> Result<()> {
    let value = value.to_string();
    instantwmctl::run([
        "mouse",
        "pointer-accel",
        value.as_str(),
        "--identifier",
        identifier,
    ])
}

/// Set the scroll factor for one `[input]` entry.
pub fn set_scroll_factor(identifier: &str, value: f64) -> Result<()> {
    let value = value.to_string();
    instantwmctl::run([
        "mouse",
        "scroll-factor",
        value.as_str(),
        "--identifier",
        identifier,
    ])
}

/// Pointer acceleration as instantWM will apply it to pointer devices, or
/// `None` when it has no value configured.
///
/// Read from `config list input` — `mouse list` answers with a debug-formatted
/// dump that is not machine-readable, not even under `--json`.
pub fn pointer_accel() -> Result<Option<f64>> {
    let values: HashMap<String, Value> = instantwmctl::json(["config", "list", "input"])?;
    // Same precedence the compositor applies, so this reports what is live.
    Ok([POINTER, WILDCARD].into_iter().find_map(|identifier| {
        values
            .get(&format!("input.{identifier}.pointer_accel"))
            .and_then(Value::as_f64)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_action_args() {
        assert_eq!(ControlAction::Disable.value(), "false");
        assert_eq!(ControlAction::Enable.value(), "true");
    }

    #[test]
    fn test_setting_control_ids() {
        assert_eq!(
            InstantWmSetting::Animated.config_key(),
            "animations.enabled"
        );
    }

    #[test]
    fn test_toggle_grammar_matches_instantwm() {
        assert_eq!(toggle(true), "enabled");
        assert_eq!(toggle(false), "disabled");
    }
}
