//! Setting trait for unified apply/restore logic
//!
//! This module defines the core `Setting` trait that all settings implement.
//! Settings are organized via the category tree in category_tree.rs.

use anyhow::Result;

use super::context::SettingsContext;
use super::store::{BoolSettingKey, StringSettingKey};
use crate::common::distro::OperatingSystem;
use crate::common::package::Dependency;
use crate::ui::prelude::NerdFont;

/// Category identifiers for settings organization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Install,
    Network,
    Bluetooth,
    Appearance,
    Mouse,
    Desktop,
    Display,
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
            Category::Display => "display",
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
            Category::Display => "Display",
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
            Category::Display => "Monitor resolution, refresh rate, and display configuration.",
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
            Category::Display => NerdFont::Monitor,
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
        use crate::ui::catppuccin::colors;
        match self {
            Category::Install => colors::BLUE,
            Category::Network => colors::GREEN,
            Category::Bluetooth => colors::BLUE,
            Category::Appearance => colors::LAVENDER,
            Category::Mouse => colors::PEACH,
            Category::Desktop => colors::MAUVE,
            Category::Display => colors::SKY,
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
            "display" => Some(Category::Display),
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

    /// All categories in display order
    pub fn all() -> &'static [Category] {
        &[
            Category::Install,
            Category::Appearance,
            Category::Display,
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
    pub icon: NerdFont,
    /// Override icon color (if None, uses category color)
    pub icon_color: Option<&'static str>,
    pub summary: &'static str,
    pub requires_reapply: bool,
    pub requirements: Vec<&'static Dependency>,
    pub supported_distros: Option<&'static [OperatingSystem]>,
}

impl SettingMetadata {
    pub fn builder() -> SettingMetadataBuilder {
        SettingMetadataBuilder::default()
    }
}

#[derive(Default)]
pub struct SettingMetadataBuilder {
    id: Option<&'static str>,
    title: Option<&'static str>,
    icon: Option<NerdFont>,
    icon_color: Option<&'static str>,
    summary: &'static str,
    requires_reapply: bool,
    requirements: Vec<&'static Dependency>,
    supported_distros: Option<&'static [OperatingSystem]>,
}

impl SettingMetadataBuilder {
    pub fn new(id: &'static str, title: &'static str, icon: NerdFont) -> Self {
        Self {
            id: Some(id),
            title: Some(title),
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

    pub fn icon(mut self, icon: NerdFont) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_color(mut self, icon_color: Option<&'static str>) -> Self {
        self.icon_color = icon_color;
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

    pub fn requirements(mut self, requirements: Vec<&'static Dependency>) -> Self {
        self.requirements = requirements;
        self
    }

    pub fn supported_distros(mut self, distros: &'static [OperatingSystem]) -> Self {
        self.supported_distros = Some(distros);
        self
    }

    pub fn build(self) -> SettingMetadata {
        let title = self.title.expect("SettingMetadata: title is required");

        SettingMetadata {
            id: self.id.expect("SettingMetadata: id is required"),
            title,
            icon: self.icon.expect("SettingMetadata: icon is required"),
            icon_color: self.icon_color,
            summary: self.summary,
            requires_reapply: self.requires_reapply,
            requirements: self.requirements,
            supported_distros: self.supported_distros,
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
/// All settings implement this trait. Settings are organized via the category tree.
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

    /// Core action logic for toggle settings.
    ///
    /// Implement this to share logic between `apply()` and `restore()`.
    /// Call from both methods to reduce code duplication.
    ///
    /// Example:
    /// ```ignore
    /// fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
    ///     let enabled = !ctx.bool(Self::KEY);
    ///     ctx.set_bool(Self::KEY, enabled);
    ///     self.apply_value(ctx, enabled)
    /// }
    /// fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
    ///     Some(self.apply_value(ctx, ctx.bool(Self::KEY)))
    /// }
    /// ```
    fn apply_value(&self, _ctx: &mut SettingsContext, _value: bool) -> Result<()> {
        Ok(())
    }

    /// Restore setting on login/reboot (if applicable)
    ///
    /// Returns None if the setting doesn't need restoration.
    fn restore(&self, _ctx: &mut SettingsContext) -> Option<Result<()>> {
        None
    }

    /// Optional shell command to run for preview content
    /// Used for action-type settings that want to show current state lazily
    /// The command should output the full preview text (including summary)
    fn preview_command(&self) -> Option<String> {
        None
    }

    /// Get the current display state of the setting
    ///
    /// By default, this computes the state based on the setting type and context usage.
    /// Override this to provide dynamic state (e.g. checking systemd service status).
    fn get_display_state(&self, ctx: &SettingsContext) -> SettingState {
        match self.setting_type() {
            SettingType::Toggle { key } => SettingState::Toggle {
                enabled: ctx.bool(key),
            },
            SettingType::Choice { key } => {
                let current = ctx.string(key);
                SettingState::Choice {
                    current_label: if current.is_empty() {
                        "Not set".to_owned()
                    } else {
                        current
                    },
                }
            }
            SettingType::Action => SettingState::Action,
            SettingType::Command => SettingState::Command,
        }
    }
}

/// State of a setting for display
#[derive(Debug, Clone)]
pub enum SettingState {
    Toggle { enabled: bool },
    Choice { current_label: String },
    Action,
    Command,
}

/// Iterate over all registered settings from the category tree
pub fn all_settings() -> impl Iterator<Item = &'static dyn Setting> {
    use crate::settings::category_tree::all_settings_from_tree;
    all_settings_from_tree().into_iter()
}

/// Find a setting by its ID
pub fn setting_by_id(id: &str) -> Option<&'static dyn Setting> {
    all_settings().find(|s| s.metadata().id == id)
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

    #[test]
    fn test_builder() {
        let metadata = SettingMetadata::builder()
            .id("test.id")
            .title("Test Title")
            .icon(NerdFont::Desktop)
            .icon_color(Some("red"))
            .summary("Test Summary")
            .requires_reapply(true)
            .build();

        assert_eq!(metadata.id, "test.id");
        assert_eq!(metadata.title, "Test Title");
        assert_eq!(metadata.icon_color, Some("red"));
        assert!(metadata.requires_reapply);
    }
}
