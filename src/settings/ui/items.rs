//! UI items for settings menu
//!
//! Display types for the FZF-based settings menu system.

use crate::menu_utils::FzfSelectable;
use crate::settings::setting::{Category, Setting};
use crate::ui::prelude::*;

use super::super::context::{format_back_icon, format_icon_colored, format_search_icon};

// ============================================================================
// Category Display
// ============================================================================

/// Display item for a category in the main menu
#[derive(Clone)]
pub struct CategoryItem {
    pub category: Category,
    pub settings: Vec<&'static dyn Setting>,
}

impl CategoryItem {
    pub fn new(category: Category, settings: Vec<&'static dyn Setting>) -> Self {
        Self { category, settings }
    }
}

/// Main menu items
#[derive(Clone)]
pub enum CategoryMenuItem {
    SearchAll,
    Category(CategoryItem),
    Close,
}

// ============================================================================
// Setting Display
// ============================================================================

/// State of a setting for display
#[derive(Clone, Copy)]
pub enum SettingState {
    Toggle { enabled: bool },
    Choice { current_label: &'static str },
    Action,
    Command,
}

/// Display item for a setting
#[derive(Clone, Copy)]
pub struct SettingItem {
    pub setting: &'static dyn Setting,
    pub state: SettingState,
}

/// Items in a category page
#[derive(Clone, Copy)]
pub enum CategoryPageItem {
    Setting(SettingItem),
    Back,
}

/// Search result item
#[derive(Clone, Copy)]
pub struct SearchItem {
    pub setting: &'static dyn Setting,
    pub state: SettingState,
}

// ============================================================================
// FzfSelectable Implementations
// ============================================================================

impl FzfSelectable for CategoryItem {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} {} ({} settings)",
            format_icon_colored(self.category.icon(), self.category.color()),
            self.category.title(),
            self.settings.len()
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        use super::super::context::{colors, hex_to_ansi_fg};

        let reset = "\x1b[0m";
        let mauve = hex_to_ansi_fg(colors::MAUVE);
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let text = hex_to_ansi_fg(colors::TEXT);
        let teal = hex_to_ansi_fg(colors::TEAL);
        let surface = hex_to_ansi_fg(colors::SURFACE1);

        let mut lines = Vec::new();

        lines.push(String::new());
        lines.push(format!(
            "{mauve}{}  {}{reset}",
            char::from(self.category.icon()),
            self.category.title()
        ));
        lines.push(format!(
            "{surface}───────────────────────────────────{reset}"
        ));
        lines.push(String::new());
        lines.push(format!("{text}{}{reset}", self.category.description()));
        lines.push(String::new());

        let preview_count = 6.min(self.settings.len());
        if preview_count > 0 {
            lines.push(format!(
                "{surface}┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄{reset}"
            ));
            lines.push(String::new());

            for (i, setting) in self.settings.iter().take(preview_count).enumerate() {
                let meta = setting.metadata();
                lines.push(format!(
                    "{teal}{} {}{reset}",
                    char::from(meta.icon),
                    meta.title
                ));
                lines.push(format!("{subtext}{}{reset}", first_line(meta.summary)));

                if i < preview_count - 1 {
                    lines.push(String::new());
                }
            }

            if self.settings.len() > preview_count {
                lines.push(String::new());
                lines.push(format!(
                    "{subtext}… and {} more{reset}",
                    self.settings.len() - preview_count
                ));
            }
        }

        crate::menu_utils::FzfPreview::Text(lines.join("\n"))
    }
}

impl FzfSelectable for CategoryMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryMenuItem::SearchAll => {
                format!("{} Search all settings", format_search_icon())
            }
            CategoryMenuItem::Category(item) => item.fzf_display_text(),
            CategoryMenuItem::Close => format!("{} Close", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            CategoryMenuItem::SearchAll => {
                use super::super::context::{colors, hex_to_ansi_fg};

                let reset = "\x1b[0m";
                let mauve = hex_to_ansi_fg(colors::MAUVE);
                let text = hex_to_ansi_fg(colors::TEXT);
                let surface = hex_to_ansi_fg(colors::SURFACE1);

                let lines = vec![
                    String::new(),
                    format!("{mauve}{}  Search All{reset}", char::from(NerdFont::Search)),
                    format!("{surface}───────────────────────────────────{reset}"),
                    String::new(),
                    format!("{text}Browse all available settings in one{reset}"),
                    format!("{text}searchable list.{reset}"),
                    String::new(),
                    format!("{text}Start typing to filter settings by{reset}"),
                    format!("{text}name, category, or description.{reset}"),
                ];

                crate::menu_utils::FzfPreview::Text(lines.join("\n"))
            }
            CategoryMenuItem::Category(item) => item.fzf_preview(),
            CategoryMenuItem::Close => {
                crate::menu_utils::FzfPreview::Text("Exit settings".to_string())
            }
        }
    }
}

impl FzfSelectable for SettingItem {
    fn fzf_display_text(&self) -> String {
        let meta = self.setting.metadata();
        let icon_color = meta.icon_color.unwrap_or_else(|| meta.category.color());

        match self.state {
            SettingState::Toggle { enabled } => {
                let status = if enabled { "[ON]" } else { "[OFF]" };
                format!(
                    "{} {} {}",
                    format_icon_colored(meta.icon, icon_color),
                    meta.title,
                    status
                )
            }
            SettingState::Choice { current_label } => {
                format!(
                    "{} {}  [{}]",
                    format_icon_colored(NerdFont::List, icon_color),
                    meta.title,
                    current_label
                )
            }
            SettingState::Action => {
                format!(
                    "{} {}",
                    format_icon_colored(meta.icon, icon_color),
                    meta.title
                )
            }
            SettingState::Command => {
                format!(
                    "{} {}",
                    format_icon_colored(NerdFont::ExternalLink, icon_color),
                    meta.title
                )
            }
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        let meta = self.setting.metadata();
        let mut lines = vec![meta.summary.to_string()];

        if let SettingState::Toggle { enabled } = self.state {
            lines.push(String::new());
            lines.push(format!(
                "Current state: {}",
                if enabled { "Enabled" } else { "Disabled" }
            ));
            lines.push(format!(
                "Select to {}.",
                if enabled { "disable" } else { "enable" }
            ));
        }

        crate::menu_utils::FzfPreview::Text(lines.join("\n"))
    }
}

impl FzfSelectable for CategoryPageItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryPageItem::Setting(item) => item.fzf_display_text(),
            CategoryPageItem::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            CategoryPageItem::Setting(item) => item.fzf_preview(),
            CategoryPageItem::Back => {
                crate::menu_utils::FzfPreview::Text("Return to categories".to_string())
            }
        }
    }
}

impl FzfSelectable for SearchItem {
    fn fzf_display_text(&self) -> String {
        let meta = self.setting.metadata();
        let path = format_setting_path(self.setting);
        let icon_color = meta.icon_color.unwrap_or_else(|| meta.category.color());

        match self.state {
            SettingState::Toggle { enabled } => {
                let status = if enabled { "[ON]" } else { "[OFF]" };
                format!(
                    "{} {} {}",
                    format_icon_colored(meta.icon, icon_color),
                    path,
                    status
                )
            }
            SettingState::Choice { current_label } => {
                format!(
                    "{} {}  [{}]",
                    format_icon_colored(NerdFont::List, icon_color),
                    path,
                    current_label
                )
            }
            SettingState::Action => {
                format!("{} {}", format_icon_colored(meta.icon, icon_color), path)
            }
            SettingState::Command => {
                format!(
                    "{} {}",
                    format_icon_colored(NerdFont::ExternalLink, icon_color),
                    path
                )
            }
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        let meta = self.setting.metadata();
        let mut lines = vec![meta.summary.to_string()];

        if let SettingState::Toggle { enabled } = self.state {
            lines.push(String::new());
            lines.push(format!(
                "Current state: {}",
                if enabled { "Enabled" } else { "Disabled" }
            ));
            lines.push(format!(
                "Select to {}.",
                if enabled { "disable" } else { "enable" }
            ));
        }

        crate::menu_utils::FzfPreview::Text(lines.join("\n"))
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

pub fn format_setting_path(setting: &dyn Setting) -> String {
    let meta = setting.metadata();
    let mut segments = Vec::with_capacity(1 + meta.breadcrumbs.len());
    segments.push(meta.category.title());
    segments.extend(meta.breadcrumbs.iter().copied());
    segments.join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::context::SettingsContext;
    use crate::settings::setting::{Category, SettingMetadata, SettingType};
    use crate::ui::prelude::NerdFont;
    use anyhow::Result;

    struct TestSetting {
        color: Option<&'static str>,
    }

    impl Setting for TestSetting {
        fn metadata(&self) -> SettingMetadata {
            SettingMetadata::builder()
                .id("test.setting")
                .title("Test Setting")
                .category(Category::Apps) // Default color is SAPPHIRE (#74c7ec)
                .icon(NerdFont::Gear)
                .icon_color(self.color)
                .summary("Test Summary")
                .build()
        }

        fn setting_type(&self) -> SettingType {
            SettingType::Action
        }

        fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_setting_item_respects_custom_color() {
        // Red background: #ff0000 -> 48;2;255;0;0
        let setting = Box::leak(Box::new(TestSetting {
            color: Some("#ff0000"),
        }));
        let item = SettingItem {
            setting,
            state: SettingState::Action,
        };

        let display = item.fzf_display_text();
        assert!(display.contains("48;2;255;0;0m"));
    }

    #[test]
    fn test_setting_item_fallback_to_category_color() {
        let setting = Box::leak(Box::new(TestSetting { color: None }));
        let item = SettingItem {
            setting,
            state: SettingState::Action,
        };

        // Category::Apps is SAPPHIRE (#74c7ec)
        // 74c7ec -> R=116, G=199, B=236
        // Expect background color set: 48;2;116;199;236
        let display = item.fzf_display_text();
        assert!(display.contains("48;2;116;199;236m"));
    }

    #[test]
    fn test_search_item_respects_custom_color() {
        // Green background: #00ff00 -> 48;2;0;255;0
        let setting = Box::leak(Box::new(TestSetting {
            color: Some("#00ff00"),
        }));
        let item = SearchItem {
            setting,
            state: SettingState::Action,
        };

        let display = item.fzf_display_text();
        assert!(display.contains("48;2;0;255;0m"));
    }
}
