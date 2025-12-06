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

    pub fn title(&self) -> &'static str {
        match self {
            Category::Install => "Installation",
            Category::Network => "Networking",
            Category::Bluetooth => "Bluetooth",
            Category::Appearance => "Appearance",
            Category::Mouse => "Mouse & Touchpad",
            Category::Desktop => "Desktop",
            Category::Audio => "Sound",
            Category::Apps => "Default Apps",
            Category::Storage => "Storage",
            Category::Printers => "Printers",
            Category::Users => "Users & Accounts",
            Category::Language => "Language & Region",
            Category::System => "System & Updates",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Category::Install => "Installation and setup options.",
            Category::Network => "WiFi, Ethernet, VPN, and network diagnostics.",
            Category::Bluetooth => "Pair devices and manage Bluetooth settings.",
            Category::Appearance => "Themes, wallpaper, brightness, and visual styles.",
            Category::Mouse => "Pointer speed, scrolling, and button settings.",
            Category::Desktop => "Desktop behaviour, window management, and layout preferences.",
            Category::Audio => "Sound routing tools and audio behaviour.",
            Category::Apps => "Default applications and file associations.",
            Category::Storage => "Disk management and auto-mounting.",
            Category::Printers => "Discover, configure, and manage printers.",
            Category::Users => "Create and manage user accounts.",
            Category::Language => "Manage system locales and language defaults.",
            Category::System => "System administration and maintenance.",
        }
    }

    pub fn icon(&self) -> NerdFont {
        match self {
            Category::Install => NerdFont::Download,
            Category::Network => NerdFont::Network,
            Category::Bluetooth => NerdFont::Bluetooth,
            Category::Appearance => NerdFont::Palette,
            Category::Mouse => NerdFont::Mouse,
            Category::Desktop => NerdFont::Desktop,
            Category::Audio => NerdFont::VolumeUp,
            Category::Apps => NerdFont::Package,
            Category::Storage => NerdFont::Database2,
            Category::Printers => NerdFont::Printer,
            Category::Users => NerdFont::Users,
            Category::Language => NerdFont::Globe,
            Category::System => NerdFont::Server,
        }
    }

    pub fn color(&self) -> &'static str {
        use super::context::colors;
        match self {
            Category::Install => colors::BLUE,
            Category::Network => colors::GREEN,
            Category::Bluetooth => colors::BLUE,
            Category::Appearance => colors::LAVENDER,
            Category::Mouse => colors::PEACH,
            Category::Desktop => colors::MAUVE,
            Category::Audio => colors::TEAL,
            Category::Apps => colors::SAPPHIRE,
            Category::Storage => colors::YELLOW,
            Category::Printers => colors::FLAMINGO,
            Category::Users => colors::MAROON,
            Category::Language => colors::ROSEWATER,
            Category::System => colors::RED,
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        match id {
            "install" => Some(Category::Install),
            "network" => Some(Category::Network),
            "bluetooth" => Some(Category::Bluetooth),
            "appearance" => Some(Category::Appearance),
            "mouse" => Some(Category::Mouse),
            "desktop" => Some(Category::Desktop),
            "audio" => Some(Category::Audio),
            "apps" => Some(Category::Apps),
            "storage" => Some(Category::Storage),
            "printers" => Some(Category::Printers),
            "users" => Some(Category::Users),
            "language" => Some(Category::Language),
            "system" => Some(Category::System),
            _ => None,
        }
    }

    /// All categories in display order (excluding Install which is special)
    pub fn all() -> &'static [Category] {
        &[
            Category::Appearance,
            Category::Network,
            Category::Bluetooth,
            Category::Mouse,
            Category::Desktop,
            Category::Audio,
            Category::Apps,
            Category::Storage,
            Category::Printers,
            Category::Users,
            Category::Language,
            Category::System,
        ]
    }
}

/// UI metadata for displaying a setting
#[derive(Debug)]
pub struct SettingMetadata {
    pub id: &'static str,
    pub title: &'static str,
    pub category: Category,
    pub icon: NerdFont,
    /// Override icon color (if None, uses category color)
    pub icon_color: Option<&'static str>,
    pub breadcrumbs: &'static [&'static str],
    pub summary: &'static str,
    pub requires_reapply: bool,
    pub requirements: &'static [Requirement],
}

impl SettingMetadata {
    pub fn builder() -> SettingMetadataBuilder {
        SettingMetadataBuilder::default()
    }
}

pub struct SettingMetadataBuilder {
    id: Option<&'static str>,
    title: Option<&'static str>,
    category: Option<Category>,
    icon: Option<NerdFont>,
    icon_color: Option<&'static str>,
    breadcrumbs: &'static [&'static str],
    summary: &'static str,
    requires_reapply: bool,
    requirements: &'static [Requirement],
}

impl Default for SettingMetadataBuilder {
    fn default() -> Self {
        Self {
            id: None,
            title: None,
            category: None,
            icon: None,
            icon_color: None,
            breadcrumbs: &[],
            summary: "",
            requires_reapply: false,
            requirements: &[],
        }
    }
}

impl SettingMetadataBuilder {
    pub fn new(
        id: &'static str,
        title: &'static str,
        category: Category,
        icon: NerdFont,
    ) -> Self {
        Self {
            id: Some(id),
            title: Some(title),
            category: Some(category),
            icon: Some(icon),
            ..Default::default()
        }
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn title(mut self, title: &'static str) -> Self {
        self.title = Some(title);
        self
    }

    pub fn category(mut self, category: Category) -> Self {
        self.category = Some(category);
        self
    }

    pub fn icon(mut self, icon: NerdFont) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_color(mut self, icon_color: &'static str) -> Self {
        self.icon_color = Some(icon_color);
        self
    }

    pub fn breadcrumbs(mut self, breadcrumbs: &'static [&'static str]) -> Self {
        self.breadcrumbs = breadcrumbs;
        self
    }

    pub fn summary(mut self, summary: &'static str) -> Self {
        self.summary = summary;
        self
    }

    pub fn requires_reapply(mut self, requires_reapply: bool) -> Self {
        self.requires_reapply = requires_reapply;
        self
    }

    pub fn requirements(mut self, requirements: &'static [Requirement]) -> Self {
        self.requirements = requirements;
        self
    }

    pub fn build(self) -> SettingMetadata {
        SettingMetadata {
            id: self.id.expect("SettingMetadata: id is required"),
            title: self.title.expect("SettingMetadata: title is required"),
            category: self.category.expect("SettingMetadata: category is required"),
            icon: self.icon.expect("SettingMetadata: icon is required"),
            icon_color: self.icon_color,
            breadcrumbs: self.breadcrumbs,
            summary: self.summary,
            requires_reapply: self.requires_reapply,
            requirements: self.requirements,
        }
    }
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
