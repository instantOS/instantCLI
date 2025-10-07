use crate::menu_utils::FzfSelectable;
use crate::ui::prelude::*;

use super::super::context::format_icon;
use super::super::registry::{SettingCategory, SettingDefinition, SettingKind, SettingOption};

#[derive(Clone, Copy)]
pub struct CategoryItem {
    pub category: &'static SettingCategory,
    pub total: usize,
    pub highlights: [Option<&'static SettingDefinition>; 3],
    //TODO: remove those fields
    pub toggles: usize,
    pub choices: usize,
    pub actions: usize,
    pub commands: usize,
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
pub struct SearchItem {
    pub category: &'static SettingCategory,
    pub definition: &'static SettingDefinition,
    pub state: SettingState,
}

impl FzfSelectable for CategoryItem {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} {} ({} settings)",
            format_icon(self.category.icon),
            self.category.title,
            self.total
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        let mut lines = Vec::new();

        lines.push(format!(
            "{} {}",
            char::from(NerdFont::Info),
            self.category.description
        ));

        let highlights: Vec<_> = self.highlights.iter().flatten().take(3).collect();

        if !highlights.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "{} Featured settings:",
                char::from(NerdFont::Lightbulb)
            ));

            for definition in highlights {
                lines.push(format!(
                    "  {} {}",
                    char::from(definition.icon),
                    definition.title,
                ));
                lines.push(format!("    {}", setting_summary(definition)));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "{} {} total setting{}",
            char::from(NerdFont::List),
            self.total,
            if self.total == 1 { "" } else { "s" }
        ));

        crate::menu_utils::FzfPreview::Text(lines.join("\n"))
    }
}

impl FzfSelectable for CategoryMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryMenuItem::SearchAll => {
                format!("{} Search all settings", format_icon(NerdFont::Search))
            }
            CategoryMenuItem::Category(item) => item.fzf_display_text(),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self {
            CategoryMenuItem::SearchAll => crate::menu_utils::FzfPreview::Text(
                "Browse and edit any available setting".to_string(),
            ),
            CategoryMenuItem::Category(item) => item.fzf_preview(),
        }
    }
}

impl FzfSelectable for SettingItem {
    fn fzf_display_text(&self) -> String {
        match self.state {
            SettingState::Toggle { enabled } => {
                let status_text = if enabled { "[ON]" } else { "[OFF]" };
                format!(
                    "{} {} {}",
                    format_icon(self.definition.icon),
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
                    format_icon(glyph),
                    self.definition.title,
                    current_label
                )
            }
            SettingState::Action => format!(
                "{} {}",
                format_icon(self.definition.icon),
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
                format!("{} {}", format_icon(glyph), self.definition.title)
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
                format!("{} Back", format_icon(NerdFont::ArrowLeft))
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

impl FzfSelectable for SearchItem {
    fn fzf_display_text(&self) -> String {
        let path = super::format_setting_path(self.category, self.definition);
        match self.state {
            SettingState::Toggle { enabled } => {
                let status_text = if enabled { "[ON]" } else { "[OFF]" };
                format!(
                    "{} {} {}",
                    format_icon(self.definition.icon),
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
                format!("{} {}  [{}]", format_icon(glyph), path, current_label)
            }
            SettingState::Action => {
                format!("{} {}", format_icon(self.definition.icon), path)
            }
            SettingState::Command => {
                let glyph = match &self.definition.kind {
                    SettingKind::Command { command, .. } => match command.style {
                        crate::settings::registry::CommandStyle::Terminal => NerdFont::Terminal,
                        crate::settings::registry::CommandStyle::Detached => NerdFont::ExternalLink,
                    },
                    _ => self.definition.icon,
                };
                format!("{} {}", format_icon(glyph), path)
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
