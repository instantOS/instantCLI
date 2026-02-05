use std::cmp::Ordering;

use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::prelude::*;

use super::state::{LocaleEntry, LocaleState};
use crate::ui::catppuccin::{colors, format_icon};

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
            LanguageMenuItem::Add => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Locale")
                .text("Enable additional locales in /etc/locale.gen.")
                .blank()
                .bullets([
                    "Select one or more locales to generate",
                    "Runs locale-gen after selection",
                ])
                .build(),
            LanguageMenuItem::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to settings.")
                .build(),
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
        let display_name = locale_display_name(&self.label, &self.locale, self.has_human_name);

        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Language, "Locale")
            .subtext("System language and formatting for applications.")
            .blank()
            .field("Locale", &self.locale);

        if let Some(name) = &display_name {
            builder = builder.field("Language", name);
        }

        builder = builder
            .blank()
            .line(colors::TEAL, Some(NerdFont::InfoCircle), "Status");

        if self.enabled {
            builder = builder.line(
                colors::GREEN,
                Some(NerdFont::CheckCircle),
                "Generated in /etc/locale.gen",
            );
        } else {
            builder = builder.line(colors::YELLOW, Some(NerdFont::Warning), "Not generated yet");
        }

        if self.is_current {
            builder = builder.line(colors::GREEN, Some(NerdFont::Check), "Current system LANG");
        }

        builder = builder.blank();
        builder = append_locale_details(builder, &self.locale);

        builder.build()
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
        match self {
            LocaleActionItem::SetDefault { label, .. } => PreviewBuilder::new()
                .header(NerdFont::Check, "Set Default Language")
                .text(&format!("Set LANG to use {label}."))
                .blank()
                .text("Log out or reboot for applications to pick it up.")
                .build(),
            LocaleActionItem::Remove { label, .. } => PreviewBuilder::new()
                .header(NerdFont::Trash, "Remove Locale")
                .text(&format!("Disable {label} in /etc/locale.gen."))
                .blank()
                .bullets([
                    "Runs locale-gen to regenerate locales",
                    "Restart apps that used this locale",
                ])
                .build(),
            LocaleActionItem::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to the locale list.")
                .build(),
        }
    }
}

#[derive(Clone)]
pub(super) struct LocaleToggleItem {
    pub(super) locale: String,
    label: String,
    has_human_name: bool,
    is_current: bool,
}

impl LocaleToggleItem {
    pub(super) fn from_entry(entry: LocaleEntry, current: Option<&str>) -> Self {
        Self {
            is_current: current == Some(entry.locale.as_str()),
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

    fn fzf_preview(&self) -> FzfPreview {
        let display_name = locale_display_name(&self.label, &self.locale, self.has_human_name);

        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Language, "Locale")
            .subtext("Generate this locale via locale-gen.")
            .blank()
            .field("Locale", &self.locale);

        if let Some(name) = &display_name {
            builder = builder.field("Language", name);
        }

        if self.is_current {
            builder = builder.line(colors::GREEN, Some(NerdFont::Check), "Matches current LANG");
        }

        builder = builder.blank();
        builder = append_locale_details(builder, &self.locale);

        builder
            .blank()
            .line(colors::TEAL, Some(NerdFont::Gear), "Action")
            .bullets([
                "Adds to /etc/locale.gen",
                "Runs locale-gen to build locale data",
            ])
            .build()
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

#[derive(Default)]
struct LocaleParts {
    language: Option<String>,
    territory: Option<String>,
    encoding: Option<String>,
    modifier: Option<String>,
}

fn locale_display_name(label: &str, locale: &str, has_human_name: bool) -> Option<String> {
    if !has_human_name {
        return None;
    }

    let suffix = format!(" ({locale})");
    Some(label.strip_suffix(&suffix).unwrap_or(label).to_string())
}

fn parse_locale_parts(locale: &str) -> LocaleParts {
    let (without_modifier, modifier) = match locale.split_once('@') {
        Some((base, modifier)) if !modifier.is_empty() => (base, Some(modifier.to_string())),
        _ => (locale, None),
    };

    let (without_encoding, encoding) = match without_modifier.split_once('.') {
        Some((base, encoding)) if !encoding.is_empty() => (base, Some(encoding.to_string())),
        _ => (without_modifier, None),
    };

    let (language, territory) = match without_encoding.split_once('_') {
        Some((language, territory)) => (
            (!language.is_empty()).then(|| language.to_string()),
            (!territory.is_empty()).then(|| territory.to_string()),
        ),
        None => (
            (!without_encoding.is_empty()).then(|| without_encoding.to_string()),
            None,
        ),
    };

    LocaleParts {
        language,
        territory,
        encoding,
        modifier,
    }
}

fn append_locale_details(mut builder: PreviewBuilder, locale: &str) -> PreviewBuilder {
    let parts = parse_locale_parts(locale);

    builder = builder.line(colors::TEAL, Some(NerdFont::Tag), "Details");

    if let Some(language) = parts.language {
        builder = builder.field_indented("Language code", &language);
    }
    if let Some(territory) = parts.territory {
        builder = builder.field_indented("Region", &territory);
    }
    if let Some(encoding) = parts.encoding {
        builder = builder.field_indented("Encoding", &encoding);
    }
    if let Some(modifier) = parts.modifier {
        builder = builder.field_indented("Variant", &modifier);
    }

    builder
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
