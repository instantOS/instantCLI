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
    InputDevices,
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

/// Metadata for a category (all properties in one place)
#[derive(Debug, Clone, Copy)]
pub struct CategoryMeta {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: NerdFont,
    pub color: &'static str,
    /// Hidden search keywords for alternative matching (e.g., "Sound" -> ["audio", "volume"])
    pub search_keywords: &'static [&'static str],
}

impl Category {
    /// Get all metadata for this category
    pub fn meta(self) -> CategoryMeta {
        use crate::ui::catppuccin::colors;
        match self {
            Category::Install => CategoryMeta {
                id: "install",
                title: "Installation",
                description: "Installation and setup options.",
                icon: NerdFont::Download,
                color: colors::BLUE,
                search_keywords: &["package"],
            },
            Category::Network => CategoryMeta {
                id: "network",
                title: "Networking",
                description: "WiFi, Ethernet, VPN, and network diagnostics.",
                icon: NerdFont::Network,
                color: colors::GREEN,
                search_keywords: &["internet", "wifi"],
            },
            Category::Bluetooth => CategoryMeta {
                id: "bluetooth",
                title: "Bluetooth",
                description: "Pair devices and manage Bluetooth settings.",
                icon: NerdFont::Bluetooth,
                color: colors::BLUE,
                search_keywords: &[],
            },
            Category::Appearance => CategoryMeta {
                id: "appearance",
                title: "Appearance",
                description: "Themes, wallpaper, brightness, and visual styles.",
                icon: NerdFont::Palette,
                color: colors::LAVENDER,
                search_keywords: &["wallpaper", "theme"],
            },
            Category::InputDevices => CategoryMeta {
                id: "input_devices",
                title: "Input Devices",
                description: "Mouse, touchpad, and keyboard settings.",
                icon: NerdFont::MousePointer,
                color: colors::PEACH,
                search_keywords: &["mouse", "keyboard"],
            },
            Category::Desktop => CategoryMeta {
                id: "desktop",
                title: "Desktop",
                description: "Desktop behaviour, window management, and layout preferences.",
                icon: NerdFont::Desktop,
                color: colors::MAUVE,
                search_keywords: &[],
            },
            Category::Display => CategoryMeta {
                id: "display",
                title: "Display",
                description: "Monitor resolution, refresh rate, and display configuration.",
                icon: NerdFont::Monitor,
                color: colors::SKY,
                search_keywords: &["monitor"],
            },
            Category::Audio => CategoryMeta {
                id: "audio",
                title: "Sound",
                description: "Sound routing tools and audio behaviour.",
                icon: NerdFont::VolumeUp,
                color: colors::TEAL,
                search_keywords: &["audio", "volume", "sound"],
            },
            Category::Apps => CategoryMeta {
                id: "apps",
                title: "Default Apps",
                description: "Default applications and file associations.",
                icon: NerdFont::Package,
                color: colors::SAPPHIRE,
                search_keywords: &[],
            },
            Category::Storage => CategoryMeta {
                id: "storage",
                title: "Storage",
                description: "Disk management and auto-mounting.",
                icon: NerdFont::Database2,
                color: colors::YELLOW,
                search_keywords: &[],
            },
            Category::Printers => CategoryMeta {
                id: "printers",
                title: "Printers",
                description: "Discover, configure, and manage printers.",
                icon: NerdFont::Printer,
                color: colors::FLAMINGO,
                search_keywords: &[],
            },
            Category::Users => CategoryMeta {
                id: "users",
                title: "Users & Accounts",
                description: "Create and manage user accounts.",
                icon: NerdFont::Users,
                color: colors::MAROON,
                search_keywords: &[],
            },
            Category::Language => CategoryMeta {
                id: "language",
                title: "Language & Region",
                description: "Manage system locales and language defaults.",
                icon: NerdFont::Globe,
                color: colors::ROSEWATER,
                search_keywords: &[],
            },
            Category::System => CategoryMeta {
                id: "system",
                title: "System & Updates",
                description: "System administration and maintenance.",
                icon: NerdFont::Server,
                color: colors::RED,
                search_keywords: &[],
            },
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        Self::all().iter().find(|c| c.meta().id == id).copied()
    }

    /// All categories in display order
    pub fn all() -> &'static [Category] {
        &[
            Category::Install,
            Category::Appearance,
            Category::Display,
            Category::Network,
            Category::Bluetooth,
            Category::InputDevices,
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
    /// Blacklist of distros where this setting should not be available
    pub unsupported_distros: Option<&'static [OperatingSystem]>,
    /// Hidden search keywords for alternative matching in fzf menus.
    /// E.g., "Sound" settings could include "audio", "volume" for discoverability.
    pub search_keywords: &'static [&'static str],
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
    unsupported_distros: Option<&'static [OperatingSystem]>,
    search_keywords: &'static [&'static str],
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

    pub fn unsupported_distros(mut self, distros: &'static [OperatingSystem]) -> Self {
        self.unsupported_distros = Some(distros);
        self
    }

    /// Set hidden search keywords for alternative matching in fzf menus.
    /// E.g., "Sound" settings could include "audio", "volume" for discoverability.
    pub fn search_keywords(mut self, keywords: &'static [&'static str]) -> Self {
        self.search_keywords = keywords;
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
            unsupported_distros: self.unsupported_distros,
            search_keywords: self.search_keywords,
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
    fn test_category_meta() {
        assert_eq!(Category::Desktop.meta().id, "desktop");
        assert_eq!(Category::Appearance.meta().id, "appearance");
        assert_eq!(Category::Desktop.meta().title, "Desktop");
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
