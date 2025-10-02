use crate::fzf_wrapper::FzfSelectable;
use crate::ui::prelude::*;

use super::super::context::format_icon;
use super::super::registry::{SettingCategory, SettingDefinition, SettingKind, SettingOption};

#[derive(Clone, Copy)]
pub struct CategoryItem {
    pub category: &'static SettingCategory,
    pub total: usize,
    pub toggles: usize,
    pub choices: usize,
    pub actions: usize,
    pub commands: usize,
    pub highlights: [Option<&'static SettingDefinition>; 3],
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

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
        let mut lines = Vec::new();

        lines.push(format!(
            "{} {}",
            char::from(Fa::InfoCircle),
            self.category.description
        ));

        let highlights: Vec<_> = self.highlights.iter().flatten().take(3).collect();

        if !highlights.is_empty() {
            lines.push(String::new());
            lines.push(format!("{} Featured settings:", char::from(Fa::LightbulbO)));

            for definition in highlights {
                lines.push(format!(
                    "  {} {}",
                    char::from(definition.icon),
                    definition.title,
                ));
                lines.push(format!(
                    "    {}",
                    setting_summary(definition)
                ));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "{} {} total setting{}",
            char::from(Fa::List),
            self.total,
            if self.total == 1 { "" } else { "s" }
        ));

        crate::fzf_wrapper::FzfPreview::Text(lines.join("\n"))
    }
}

impl FzfSelectable for CategoryMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryMenuItem::SearchAll => {
                format!("{} Search all settings", format_icon(Fa::Search))
            }
            CategoryMenuItem::Category(item) => item.fzf_display_text(),
        }
    }

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
        match self {
            CategoryMenuItem::SearchAll => crate::fzf_wrapper::FzfPreview::Text(
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
                let glyph = if enabled { Fa::ToggleOn } else { Fa::ToggleOff };
                format!("{} {}", format_icon(glyph), self.definition.title)
            }
            SettingState::Choice { current_index } => {
                let glyph = Fa::List;
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
                        crate::settings::registry::CommandStyle::Terminal => Fa::Terminal,
                        crate::settings::registry::CommandStyle::Detached => Fa::ExternalLink,
                    },
                    _ => self.definition.icon,
                };
                format!("{} {}", format_icon(glyph), self.definition.title)
            }
        }
    }

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
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

                crate::fzf_wrapper::FzfPreview::Text(lines.join("\n"))
            }
        }
    }
}

impl FzfSelectable for CategoryPageItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryPageItem::Setting(item) => item.fzf_display_text(),
            CategoryPageItem::Back => {
                format!("{} Back", format_icon(Fa::ArrowLeft))
            }
        }
    }

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
        match self {
            CategoryPageItem::Setting(item) => item.fzf_preview(),
            CategoryPageItem::Back => {
                crate::fzf_wrapper::FzfPreview::Text("Return to categories".to_string())
            }
        }
    }
}

impl FzfSelectable for ChoiceItem {
    fn fzf_display_text(&self) -> String {
        let glyph = if self.is_current {
            Fa::CheckSquareO
        } else {
            Fa::SquareO
        };
        format!("{} {}", format_icon(glyph), self.option.label)
    }

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
        crate::fzf_wrapper::FzfPreview::Text(format!(
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
                let glyph = if enabled { Fa::ToggleOn } else { Fa::ToggleOff };
                format!("{} {}", format_icon(glyph), path)
            }
            SettingState::Choice { current_index } => {
                let glyph = Fa::List;
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
                        crate::settings::registry::CommandStyle::Terminal => Fa::Terminal,
                        crate::settings::registry::CommandStyle::Detached => Fa::ExternalLink,
                    },
                    _ => self.definition.icon,
                };
                format!("{} {}", format_icon(glyph), path)
            }
        }
    }

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
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

                crate::fzf_wrapper::FzfPreview::Text(lines.join("\n"))
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
