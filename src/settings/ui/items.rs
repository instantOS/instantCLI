use crate::menu_utils::FzfSelectable;
use crate::ui::prelude::*;

use super::super::context::{format_back_icon, format_icon, format_icon_colored, format_search_icon};
use super::super::registry::{SettingCategory, SettingDefinition, SettingKind, SettingOption, category_by_id};

#[derive(Clone, Copy)]
pub struct CategoryItem {
    pub category: &'static SettingCategory,
    pub total: usize,
    pub highlights: [Option<&'static SettingDefinition>; 6],
}

#[derive(Clone, Copy)]
pub enum CategoryMenuItem {
    SearchAll,
    Category(CategoryItem),
}

#[derive(Clone, Copy)]
pub struct SettingItem {
    pub definition: &'static SettingDefinition,
    pub state: SettingState,
}

#[derive(Clone, Copy)]
pub enum CategoryPageItem {
    Setting(SettingItem),
    Back,
}

#[derive(Clone, Copy)]
pub enum SettingState {
    Toggle { enabled: bool },
    Choice { current_index: Option<usize> },
    Action,
    Command,
}

#[derive(Clone, Copy)]
pub struct ChoiceItem {
    pub option: &'static SettingOption,
    pub is_current: bool,
    pub summary: &'static str,
}

#[derive(Clone, Copy)]
pub enum ChoiceMenuItem {
    Option(ChoiceItem),
    Back,
}

#[derive(Clone, Copy)]
pub struct SearchItem {
    pub category: &'static SettingCategory,
    pub definition: &'static SettingDefinition,
    pub state: SettingState,
}

impl FzfSelectable for CategoryItem {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} {} ({} settings)",
            format_icon_colored(self.category.icon, self.category.icon_color),
            self.category.title,
            self.total
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        use super::super::context::{colors, hex_to_ansi_fg};

        let reset = "\x1b[0m";
        let mauve = hex_to_ansi_fg(colors::MAUVE);
        let overlay = hex_to_ansi_fg(colors::OVERLAY0);
        let text = hex_to_ansi_fg(colors::TEXT);
        let teal = hex_to_ansi_fg(colors::TEAL);
        let surface = hex_to_ansi_fg(colors::SURFACE1);

        let mut lines = Vec::new();

        // Top padding
        lines.push(String::new());

        // Header with category title (mauve colored)
        lines.push(format!(
            "{mauve}{}  {}{reset}",
            char::from(self.category.icon),
            self.category.title
        ));
        lines.push(format!("{surface}───────────────────────────────────{reset}"));
        lines.push(String::new());

        // Description (text colored)
        lines.push(format!("{text}{}{reset}", self.category.description));
        lines.push(String::new());

        // Show settings in this category
        let all_settings: Vec<_> = self.highlights.iter().flatten().collect();

        if !all_settings.is_empty() {
            // Separator before settings
            lines.push(format!("{surface}┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄{reset}"));
            lines.push(String::new());

            for (i, definition) in all_settings.iter().enumerate() {
                // Setting title with icon (teal colored)
                lines.push(format!(
                    "{teal}{} {}{reset}",
                    char::from(definition.icon),
                    definition.title
                ));
                // Setting summary (overlay colored, no indent)
                lines.push(format!("{overlay}{}{reset}", setting_summary(definition)));

                // Spacing between settings
                if i < all_settings.len() - 1 {
                    lines.push(String::new());
                }
            }

            // Show count if there are more settings not listed
            if self.total > all_settings.len() {
                lines.push(String::new());
                lines.push(format!(
                    "{overlay}… and {} more{reset}",
                    self.total - all_settings.len()
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

                let mut lines = Vec::new();

                // Top padding
                lines.push(String::new());

                lines.push(format!(
                    "{mauve}{}  Search All{reset}",
                    char::from(NerdFont::Search)
                ));
                lines.push(format!("{surface}───────────────────────────────────{reset}"));
                lines.push(String::new());
                lines.push(format!("{text}Browse all available settings in one{reset}"));
                lines.push(format!("{text}searchable list.{reset}"));
                lines.push(String::new());
                lines.push(format!("{text}Start typing to filter settings by{reset}"));
                lines.push(format!("{text}name, category, or description.{reset}"));

                crate::menu_utils::FzfPreview::Text(lines.join("\n"))
            }
            CategoryMenuItem::Category(item) => item.fzf_preview(),
        }
    }
}

impl FzfSelectable for SettingItem {
    fn fzf_display_text(&self) -> String {
        let icon_color = category_by_id(self.definition.category)
            .map(|c| c.icon_color)
            .unwrap_or(super::super::context::colors::BLUE);
        match self.state {
            SettingState::Toggle { enabled } => {
                let status_text = if enabled { "[ON]" } else { "[OFF]" };
                format!(
                    "{} {} {}",
                    format_icon_colored(self.definition.icon, icon_color),
                    self.definition.title,
                    status_text
                )
            }
            SettingState::Choice { current_index } => {
                let glyph = NerdFont::List;
                let current_label =
                    if let SettingKind::Choice { options, .. } = &self.definition.kind {
                        current_index
                            .and_then(|index| options.get(index))
                            .map(|option| option.label)
                            .unwrap_or("Not set")
                    } else {
                        "Not set"
                    };
                format!(
                    "{} {}  [{}]",
                    format_icon_colored(glyph, icon_color),
                    self.definition.title,
                    current_label
                )
            }
            SettingState::Action => format!(
                "{} {}",
                format_icon_colored(self.definition.icon, icon_color),
                self.definition.title
            ),
            SettingState::Command => {
                let glyph = match &self.definition.kind {
                    SettingKind::Command { command, .. } => match command.style {
                        crate::settings::registry::CommandStyle::Terminal => NerdFont::Terminal,
                        crate::settings::registry::CommandStyle::Detached => NerdFont::ExternalLink,
                    },
                    _ => self.definition.icon,
                };
                format!("{} {}", format_icon_colored(glyph, icon_color), self.definition.title)
            }
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match &self.definition.kind {
            SettingKind::Toggle { summary, .. }
            | SettingKind::Choice { summary, .. }
            | SettingKind::Action { summary, .. }
            | SettingKind::Command { summary, .. } => {
                let mut lines = vec![summary.to_string()];

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
    }
}

impl FzfSelectable for CategoryPageItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryPageItem::Setting(item) => item.fzf_display_text(),
            CategoryPageItem::Back => {
                format!("{} Back", format_back_icon())
            }
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

impl FzfSelectable for ChoiceItem {
    fn fzf_display_text(&self) -> String {
        let glyph = if self.is_current {
            NerdFont::CheckSquare
        } else {
            NerdFont::Square
        };
        let status_text = if self.is_current { "[✓]" } else { "[ ]" };
        format!(
            "{} {} {}",
            format_icon(glyph),
            self.option.label,
            status_text
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(format!(
            "{}\n\n{}",
            self.option.description, self.summary
        ))
    }
}

impl FzfSelectable for ChoiceMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            ChoiceMenuItem::Option(item) => item.fzf_display_text(),
            ChoiceMenuItem::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            ChoiceMenuItem::Option(item) => item.fzf_preview(),
            ChoiceMenuItem::Back => {
                crate::menu_utils::FzfPreview::Text("Return to settings".to_string())
            }
        }
    }
}

impl FzfSelectable for SearchItem {
    fn fzf_display_text(&self) -> String {
        let path = super::format_setting_path(self.category, self.definition);
        let icon_color = self.category.icon_color;
        match self.state {
            SettingState::Toggle { enabled } => {
                let status_text = if enabled { "[ON]" } else { "[OFF]" };
                format!(
                    "{} {} {}",
                    format_icon_colored(self.definition.icon, icon_color),
                    path,
                    status_text
                )
            }
            SettingState::Choice { current_index } => {
                let glyph = NerdFont::List;
                let current_label =
                    if let SettingKind::Choice { options, .. } = &self.definition.kind {
                        current_index
                            .and_then(|index| options.get(index))
                            .map(|option| option.label)
                            .unwrap_or("Not set")
                    } else {
                        "Not set"
                    };
                format!("{} {}  [{}]", format_icon_colored(glyph, icon_color), path, current_label)
            }
            SettingState::Action => {
                format!("{} {}", format_icon_colored(self.definition.icon, icon_color), path)
            }
            SettingState::Command => {
                let glyph = match &self.definition.kind {
                    SettingKind::Command { command, .. } => match command.style {
                        crate::settings::registry::CommandStyle::Terminal => NerdFont::Terminal,
                        crate::settings::registry::CommandStyle::Detached => NerdFont::ExternalLink,
                    },
                    _ => self.definition.icon,
                };
                format!("{} {}", format_icon_colored(glyph, icon_color), path)
            }
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match &self.definition.kind {
            SettingKind::Toggle { summary, .. }
            | SettingKind::Choice { summary, .. }
            | SettingKind::Action { summary, .. }
            | SettingKind::Command { summary, .. } => {
                let mut lines = vec![summary.to_string()];

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
    }
}

pub fn setting_summary(definition: &SettingDefinition) -> &'static str {
    match &definition.kind {
        SettingKind::Toggle { summary, .. } => summary,
        SettingKind::Choice { summary, .. } => summary,
        SettingKind::Action { summary, .. } => summary,
        SettingKind::Command { summary, .. } => summary,
    }
}
