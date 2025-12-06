//! Setting trait for unified apply/restore logic
//!
//! This module defines the core `Setting` trait that all settings implement.
//! Settings are registered at compile time using the `inventory` crate.

use anyhow::Result;

use super::context::SettingsContext;
use super::store::{BoolSettingKey, StringSettingKey};
use crate::common::requirements::RequiredPackage;
use crate::ui::prelude::NerdFont;

/// Requirement that must be satisfied before a setting can be used
#[derive(Debug, Clone)]
pub enum Requirement {
    /// Requires a package to be installed
    Package(RequiredPackage),
    /// Custom runtime condition
    Condition {
        description: &'static str,
        check: fn() -> bool,
        resolve_hint: &'static str,
    },
}

impl Requirement {
    pub fn is_satisfied(&self) -> bool {
        match self {
            Requirement::Package(pkg) => pkg.is_installed(),
            Requirement::Condition { check, .. } => check(),
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Requirement::Package(pkg) => pkg.name,
            Requirement::Condition { description, .. } => description,
        }
    }

    pub fn resolve_hint(&self) -> String {
        match self {
            Requirement::Package(pkg) => pkg.install_hint(),
            Requirement::Condition { resolve_hint, .. } => resolve_hint.to_string(),
        }
    }
}

/// Category identifiers for settings organization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Install,
    Network,
    Bluetooth,
    Appearance,
    Mouse,
    Desktop,
    Audio,
    Apps,
    Storage,
    Printers,
    Users,
    Language,
    System,
}

impl Category {
    pub fn id(&self) -> &'static str {
        match self {
            Category::Install => "install",
            Category::Network => "network",
            Category::Bluetooth => "bluetooth",
            Category::Appearance => "appearance",
            Category::Mouse => "mouse",
            Category::Desktop => "desktop",
            Category::Audio => "audio",
            Category::Apps => "apps",
            Category::Storage => "storage",
            Category::Printers => "printers",
            Category::Users => "users",
            Category::Language => "language",
            Category::System => "system",
        }
    }
}

/// UI metadata for displaying a setting
#[derive(Debug)]
pub struct SettingMetadata {
    pub id: &'static str,
    pub title: &'static str,
    pub category: Category,
    pub icon: NerdFont,
    pub breadcrumbs: &'static [&'static str],
    pub summary: &'static str,
    pub requires_reapply: bool,
    pub requirements: &'static [Requirement],
}

/// The kind of setting, determining how it's displayed and interacted with
#[derive(Debug, Clone, Copy)]
pub enum SettingType {
    /// On/off toggle with a boolean key
    Toggle { key: BoolSettingKey },
    /// Multiple choice selection with a string key
    Choice { key: StringSettingKey },
    /// Action that runs a function (may or may not store state)
    Action,
    /// Command that launches an external program
    Command,
}

/// A setting that can be applied and optionally restored
///
/// All settings implement this trait. Settings are registered at compile time
/// using the `inventory` crate.
///
/// # Example
///
/// ```rust,ignore
/// pub struct SwapEscape;
///
/// impl Setting for SwapEscape {
///     fn metadata(&self) -> SettingMetadata {
///         SettingMetadata {
///             id: "desktop.swap_escape",
///             title: "Swap Escape and Caps Lock",
///             category: Category::Desktop,
///             // ...
///         }
///     }
///
///     fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
///         // Toggle and apply
///     }
///
///     fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
///         Some(restore_impl(ctx))
///     }
/// }
/// ```
pub trait Setting: Send + Sync + 'static {
    /// UI metadata for the settings panel
    fn metadata(&self) -> SettingMetadata;

    /// The type of setting (toggle, choice, action, command)
    fn setting_type(&self) -> SettingType;

    /// Apply this setting (called when user interacts)
    fn apply(&self, ctx: &mut SettingsContext) -> Result<()>;

    /// Restore setting on login/reboot (if applicable)
    /// Returns None if the setting doesn't need restoration
    fn restore(&self, _ctx: &mut SettingsContext) -> Option<Result<()>> {
        None
    }
}

// Register settings at compile time
inventory::collect!(&'static dyn Setting);

/// Iterate over all registered settings
pub fn all_settings() -> impl Iterator<Item = &'static dyn Setting> {
    inventory::iter::<&'static dyn Setting>.into_iter().copied()
}

/// Find a setting by its ID
pub fn setting_by_id(id: &str) -> Option<&'static dyn Setting> {
    all_settings().find(|s| s.metadata().id == id)
}

/// Get all settings in a category
pub fn settings_in_category(category: Category) -> Vec<&'static dyn Setting> {
    all_settings()
        .filter(|s| s.metadata().category == category)
        .collect()
}

/// Get all settings that require reapply on login
pub fn settings_requiring_reapply() -> impl Iterator<Item = &'static dyn Setting> {
    all_settings().filter(|s| s.metadata().requires_reapply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_id() {
        assert_eq!(Category::Desktop.id(), "desktop");
        assert_eq!(Category::Appearance.id(), "appearance");
    }
}
