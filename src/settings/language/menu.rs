use std::cmp::Ordering;

use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::prelude::*;

use super::state::{LocaleEntry, LocaleState};
use crate::ui::catppuccin::format_icon;

#[derive(Clone)]
pub(super) enum LanguageMenuItem {
    Locale(LocaleMenuEntry),
    Add,
    Back,
}

impl FzfSelectable for LanguageMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            LanguageMenuItem::Locale(entry) => entry.fzf_display_text(),
            LanguageMenuItem::Add => format!("{} Add locale", format_icon(NerdFont::Plus)),
            LanguageMenuItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            LanguageMenuItem::Locale(entry) => entry.fzf_preview(),
            LanguageMenuItem::Add => FzfPreview::Text(
                "Enable additional locales by writing to /etc/locale.gen and running locale-gen"
                    .to_string(),
            ),
            LanguageMenuItem::Back => FzfPreview::Text("Return to settings".to_string()),
        }
    }
}

#[derive(Clone)]
pub(super) struct LocaleMenuEntry {
    pub(super) locale: String,
    pub(super) label: String,
    pub(super) is_current: bool,
    pub(super) has_human_name: bool,
    pub(super) enabled: bool,
}

impl LocaleMenuEntry {
    pub(super) fn from_entry(entry: LocaleEntry, current: Option<&str>) -> Self {
        Self {
            is_current: current == Some(entry.locale.as_str()),
            has_human_name: entry.has_human_name,
            enabled: entry.enabled,
            locale: entry.locale,
            label: entry.label,
        }
    }

    fn fzf_display_text(&self) -> String {
        let marker = if self.is_current {
            format!("{} ", char::from(NerdFont::Check))
        } else {
            "- ".to_string()
        };

        format!("{marker}{}", self.label)
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut lines = vec![format!(
            "{} Locale: {}",
            char::from(NerdFont::Info),
            self.locale
        )];

        if self.is_current {
            lines.push(format!(
                "{} This is the current system language (LANG).",
                char::from(NerdFont::Check)
            ));
        }

        lines.push(if self.enabled {
            format!(
                "{} Generated locale present in /etc/locale.gen",
                char::from(NerdFont::CheckCircle)
            )
        } else {
            format!(
                "{} Locale not yet generated; add it to /etc/locale.gen",
                char::from(NerdFont::Warning)
            )
        });

        FzfPreview::Text(lines.join("\n"))
    }
}

impl PartialEq for LocaleMenuEntry {
    fn eq(&self, other: &Self) -> bool {
        self.locale == other.locale
    }
}

impl Eq for LocaleMenuEntry {}

impl PartialOrd for LocaleMenuEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LocaleMenuEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.has_human_name, other.has_human_name) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => self.label.cmp(&other.label),
        }
    }
}

#[derive(Clone)]
pub(super) enum LocaleActionItem {
    SetDefault { locale: String, label: String },
    Remove { locale: String, label: String },
    Back,
}

impl FzfSelectable for LocaleActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            LocaleActionItem::SetDefault { .. } => {
                format!("{} Set as default language", format_icon(NerdFont::Check))
            }
            LocaleActionItem::Remove { .. } => {
                format!("{} Remove locale", format_icon(NerdFont::Trash))
            }
            LocaleActionItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let text = match self {
            LocaleActionItem::SetDefault { label, .. } => {
                format!("Set LANG to use {label} as the system language.")
            }
            LocaleActionItem::Remove { label, .. } => {
                format!("Comment {label} out of /etc/locale.gen and regenerate locales.")
            }
            LocaleActionItem::Back => "Return to the locale list".to_string(),
        };
        FzfPreview::Text(text)
    }
}

#[derive(Clone)]
pub(super) struct LocaleToggleItem {
    pub(super) locale: String,
    label: String,
    has_human_name: bool,
}

impl LocaleToggleItem {
    pub(super) fn from_entry(entry: LocaleEntry) -> Self {
        Self {
            locale: entry.locale,
            label: entry.label,
            has_human_name: entry.has_human_name,
        }
    }
}

impl FzfSelectable for LocaleToggleItem {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }
}

impl PartialEq for LocaleToggleItem {
    fn eq(&self, other: &Self) -> bool {
        self.locale == other.locale
    }
}

impl Eq for LocaleToggleItem {}

impl PartialOrd for LocaleToggleItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LocaleToggleItem {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.has_human_name, other.has_human_name) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => self.label.cmp(&other.label),
        }
    }
}

pub(super) fn build_language_menu_items(state: &LocaleState) -> Vec<LanguageMenuItem> {
    let current = state.current_locale();
    let mut entries: Vec<LocaleMenuEntry> = state
        .entries()
        .iter()
        .filter(|entry| {
            entry.enabled
                || current
                    .map(|locale| locale == entry.locale.as_str())
                    .unwrap_or(false)
        })
        .cloned()
        .map(|entry| LocaleMenuEntry::from_entry(entry, current))
        .collect();

    entries.sort();

    let mut items: Vec<LanguageMenuItem> =
        entries.into_iter().map(LanguageMenuItem::Locale).collect();

    items.push(LanguageMenuItem::Add);
    items.push(LanguageMenuItem::Back);

    items
}
