//! # Annotations Module
//!
//! This module provides human-readable annotations for values which might not be
//! trivial to understand for new users. It enhances user experience by adding
//! descriptive context to technical values like locale codes, keymap names, etc.
//!
//! ## Features
//!
//! - **AnnotatedValue<T>**: A wrapper that adds optional annotations to any value
//! - **AnnotationProvider trait**: Allows custom annotation logic for different value types
//! - **Built-in providers**: Pre-configured annotations for locales, keymaps, and timezones
//! - **Dynamic names**: Locale names come from the system's i18n locale definitions and
//!   keymap names from the XKB layout registry; curated tables act as fast, dependency-free
//!   fallbacks for the most common entries
//! - **Detection**: Each provider also knows the current system's configured value and
//!   marks that entry as detected, so users can spot the value they most likely want
//! - **FZF integration**: Seamless integration with the fuzzy finder UI
//! - **Sorting support**: Prioritizes annotated values in UI listings
//!
//! ## Examples
//!
//! ```rust
//! // Create annotated values
//! let locale = AnnotatedValue::new(
//!     "de_DE.UTF-8".to_string(),
//!     Some("German (Germany)".to_string())
//! );
//!
//! // Display in FZF: "German (Germany) - de_DE.UTF-8"
//! ```

use crate::menu_utils::{FzfPreview, FzfSelectable};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
#[derive(Debug, Clone)]
pub struct AnnotatedValue<T> {
    pub value: T,
    pub annotation: Option<String>,
}

impl<T> AnnotatedValue<T> {
    pub fn new(value: T, annotation: Option<String>) -> Self {
        Self { value, annotation }
    }
}

impl<T: PartialEq> PartialEq for AnnotatedValue<T> {
    fn eq(&self, other: &Self) -> bool {
        self.annotation == other.annotation && self.value == other.value
    }
}

impl<T: Eq> Eq for AnnotatedValue<T> {}

impl<T: PartialOrd> PartialOrd for AnnotatedValue<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (&self.annotation, &other.annotation) {
            (Some(a), Some(b)) => {
                let ann_cmp = a.cmp(b);
                if ann_cmp != std::cmp::Ordering::Equal {
                    return Some(ann_cmp);
                }
                self.value.partial_cmp(&other.value)
            }
            (Some(_), None) => Some(std::cmp::Ordering::Less),
            (None, Some(_)) => Some(std::cmp::Ordering::Greater),
            (None, None) => self.value.partial_cmp(&other.value),
        }
    }
}

impl<T: Ord> Ord for AnnotatedValue<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (&self.annotation, &other.annotation) {
            (Some(a), Some(b)) => {
                let ann_cmp = a.cmp(b);
                if ann_cmp != std::cmp::Ordering::Equal {
                    return ann_cmp;
                }
                self.value.cmp(&other.value)
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => self.value.cmp(&other.value),
        }
    }
}

impl<T: FzfSelectable> FzfSelectable for AnnotatedValue<T> {
    fn fzf_display_text(&self) -> String {
        match &self.annotation {
            Some(ann) => format!("{} - {}", ann, self.value.fzf_display_text()),
            None => self.value.fzf_display_text(),
        }
    }

    fn fzf_key(&self) -> String {
        self.value.fzf_key()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.value.fzf_preview()
    }
}

pub trait AnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String>;
}

pub fn annotate_list<T: FzfSelectable + Clone + Ord>(
    provider: Option<&dyn AnnotationProvider>,
    items: Vec<T>,
) -> Vec<AnnotatedValue<T>> {
    let mut list: Vec<AnnotatedValue<T>> = items
        .into_iter()
        .map(|item| {
            let annotation = if let Some(p) = provider {
                let key = item.fzf_key();
                p.annotate(&key)
            } else {
                None
            };
            AnnotatedValue::new(item, annotation)
        })
        .collect();

    list.sort();
    list
}

/// Curated locale names. Checked before the dynamic i18n lookup because these
/// read better than the raw `LC_IDENTIFICATION` titles, and they keep working
/// on systems without `/usr/share/i18n`.
static CURATED_LOCALE_NAMES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("en_US.UTF-8", "English (United States)"),
        ("en_GB.UTF-8", "English (United Kingdom)"),
        ("de_DE.UTF-8", "German (Germany)"),
        ("fr_FR.UTF-8", "French (France)"),
        ("es_ES.UTF-8", "Spanish (Spain)"),
        ("it_IT.UTF-8", "Italian (Italy)"),
        ("pt_BR.UTF-8", "Portuguese (Brazil)"),
        ("ru_RU.UTF-8", "Russian (Russia)"),
        ("ja_JP.UTF-8", "Japanese (Japan)"),
        ("zh_CN.UTF-8", "Chinese (China)"),
    ])
});

/// Curated keymap names. Checked before the XKB registry so the most common
/// console keymaps stay annotated even without `/usr/share/X11`.
static CURATED_KEYMAP_NAMES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("us", "English (US)"),
        ("de-latin1", "German"),
        ("uk", "English (UK)"),
        ("fr", "French"),
        ("es", "Spanish"),
        ("it", "Italian"),
        ("pt-latin1", "Portuguese"),
        ("ru", "Russian"),
        ("jp106", "Japanese"),
    ])
});

/// Human-readable names for XKB layouts, loaded once from the layout registry.
/// Empty when `/usr/share/X11/xkb/rules/evdev.lst` is unavailable.
static XKB_LAYOUT_NAMES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    crate::settings::definitions::keyboard::parse_xkb_layouts()
        .unwrap_or_default()
        .into_iter()
        .map(|layout| (layout.code, layout.name))
        .collect()
});

/// Display names for locale definition files, parsed from
/// `/usr/share/i18n/locales` on demand. Caching is keyed by the locale base
/// (e.g. `de_DE` for `de_DE.UTF-8`); `None` is cached negatively so missing
/// files are not re-read for every list entry.
static LOCALE_DISPLAY_NAMES: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn dynamic_locale_display_name(locale: &str) -> Option<String> {
    let base = crate::settings::language::locale_base(locale);

    let mut cache = LOCALE_DISPLAY_NAMES.lock().unwrap();
    if let Some(cached) = cache.get(base) {
        return cached.clone();
    }

    let display_name = fs::read_to_string(Path::new("/usr/share/i18n/locales").join(base))
        .ok()
        .and_then(|contents| crate::settings::language::locale_display_name(&contents));
    cache.insert(base.to_string(), display_name.clone());
    display_name
}

/// Map a console keymap name to its XKB layout code, tolerating the naming
/// differences between the two registries (`de-latin1` -> `de`,
/// `uk` -> `gb`, `jp106` -> `jp`, `it2` -> `it`, ...).
fn console_keymap_to_xkb(keymap: &str, layouts: &HashMap<String, String>) -> Option<String> {
    /// Console keymaps whose XKB counterpart is not derivable by rule.
    const ALIASES: &[(&str, &str)] = &[("uk", "gb"), ("jp106", "jp"), ("sv-latin1", "se")];

    let known = |code: &str| layouts.contains_key(code);

    if let Some((_, code)) = ALIASES.iter().find(|(from, _)| *from == keymap)
        && known(code)
    {
        return Some((*code).to_string());
    }

    if known(keymap) {
        return Some(keymap.to_string());
    }

    if let Some((prefix, _)) = keymap.split_once('-')
        && known(prefix)
    {
        return Some(prefix.to_string());
    }

    let without_digits = keymap.trim_end_matches(|c: char| c.is_ascii_digit());
    if without_digits.len() != keymap.len() && known(without_digits) {
        return Some(without_digits.to_string());
    }

    None
}

fn keymap_display_name(keymap: &str) -> Option<String> {
    if let Some(name) = CURATED_KEYMAP_NAMES.get(keymap) {
        return Some((*name).to_string());
    }

    console_keymap_to_xkb(keymap, &XKB_LAYOUT_NAMES)
        .and_then(|code| XKB_LAYOUT_NAMES.get(&code).cloned())
}

/// Combine a resolved display name with the "detected" marker for the value
/// the current system is configured to use.
fn annotate_with_detection(
    name: Option<String>,
    value: &str,
    detected: &Option<String>,
) -> Option<String> {
    let is_detected = detected.as_deref() == Some(value);
    match name {
        Some(name) if is_detected => Some(format!("{name} (detected)")),
        Some(name) => Some(name),
        None if is_detected => Some("Detected (current)".to_string()),
        None => None,
    }
}

pub struct LocaleAnnotationProvider {
    detected: Option<String>,
}

impl LocaleAnnotationProvider {
    pub fn new() -> Self {
        Self {
            detected: crate::arch::locales::detect_current_locale(),
        }
    }
}

impl Default for LocaleAnnotationProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AnnotationProvider for LocaleAnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String> {
        let name = CURATED_LOCALE_NAMES
            .get(value)
            .map(|name| (*name).to_string())
            .or_else(|| dynamic_locale_display_name(value));

        annotate_with_detection(name, value, &self.detected)
    }
}

pub struct KeymapAnnotationProvider {
    detected: Option<String>,
}

impl KeymapAnnotationProvider {
    pub fn new() -> Self {
        Self {
            detected: crate::arch::keymaps::detect_current_keymap(),
        }
    }
}

impl Default for KeymapAnnotationProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AnnotationProvider for KeymapAnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String> {
        let name = keymap_display_name(value);

        annotate_with_detection(name, value, &self.detected)
    }
}

pub struct TimezoneAnnotationProvider {
    detected: Option<String>,
}

impl TimezoneAnnotationProvider {
    pub fn new() -> Self {
        Self {
            detected: crate::arch::timezones::detect_current_timezone(),
        }
    }
}

impl Default for TimezoneAnnotationProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AnnotationProvider for TimezoneAnnotationProvider {
    fn annotate(&self, value: &str) -> Option<String> {
        // Raw `Region/City` values are readable on their own; the annotation
        // exists to flag the value the running system already uses, which the
        // annotated-first sorting floats to the top of the list.
        annotate_with_detection(None, value, &self.detected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annotated_value_display() {
        let val = AnnotatedValue::new("de_DE.UTF-8", Some("German (Germany)".to_string()));
        assert_eq!(val.fzf_display_text(), "German (Germany) - de_DE.UTF-8");

        let val_no_ann = AnnotatedValue::new("unknown", None);
        assert_eq!(val_no_ann.fzf_display_text(), "unknown");
    }

    #[test]
    fn test_annotate_list() {
        let provider = LocaleAnnotationProvider { detected: None };
        let items = vec!["unknown", "de_DE.UTF-8"];
        let annotated = annotate_list(Some(&provider), items);

        assert_eq!(annotated.len(), 2);
        // Annotated should come first
        assert_eq!(
            annotated[0].fzf_display_text(),
            "German (Germany) - de_DE.UTF-8"
        );
        assert_eq!(annotated[1].fzf_display_text(), "unknown");
    }

    #[test]
    fn test_annotate_list_sorting() {
        let provider = LocaleAnnotationProvider { detected: None };
        // "en_US.UTF-8" -> "English (United States)"
        // "de_DE.UTF-8" -> "German (Germany)"
        // "unknown1"
        // "unknown2"
        let items = vec!["unknown2", "en_US.UTF-8", "unknown1", "de_DE.UTF-8"];
        let annotated = annotate_list(Some(&provider), items);

        assert_eq!(annotated.len(), 4);

        // Expected order:
        // 1. English (United States) (Annotated, 'E' < 'G')
        // 2. German (Germany) (Annotated)
        // 3. unknown1 (Non-annotated, 'u1' < 'u2')
        // 4. unknown2 (Non-annotated)
        assert_eq!(annotated[0].value, "en_US.UTF-8");
        assert_eq!(annotated[1].value, "de_DE.UTF-8");
        assert_eq!(annotated[2].value, "unknown1");
        assert_eq!(annotated[3].value, "unknown2");
    }

    #[test]
    fn test_annotate_list_no_provider() {
        let items = vec!["de_DE.UTF-8", "unknown"];
        let annotated = annotate_list(None, items);

        assert_eq!(annotated.len(), 2);
        assert_eq!(annotated[0].fzf_display_text(), "de_DE.UTF-8");
        assert_eq!(annotated[1].fzf_display_text(), "unknown");
    }

    #[test]
    fn detection_marker_is_appended_to_resolved_names() {
        assert_eq!(
            annotate_with_detection(
                Some("German (Germany)".to_string()),
                "de_DE.UTF-8",
                &Some("de_DE.UTF-8".to_string())
            ),
            Some("German (Germany) (detected)".to_string())
        );
        assert_eq!(
            annotate_with_detection(
                Some("German (Germany)".to_string()),
                "de_DE.UTF-8",
                &Some("fr_FR.UTF-8".to_string())
            ),
            Some("German (Germany)".to_string())
        );
    }

    #[test]
    fn values_without_a_name_are_annotated_only_when_detected() {
        assert_eq!(
            annotate_with_detection(None, "Europe/Berlin", &Some("Europe/Berlin".to_string())),
            Some("Detected (current)".to_string())
        );
        assert_eq!(annotate_with_detection(None, "Europe/Berlin", &None), None);
        assert_eq!(
            annotate_with_detection(None, "Europe/Berlin", &Some("UTC".to_string())),
            None
        );
    }

    #[test]
    fn timezone_provider_annotates_only_the_detected_timezone() {
        let provider = TimezoneAnnotationProvider {
            detected: Some("Europe/Berlin".to_string()),
        };

        assert_eq!(
            provider.annotate("Europe/Berlin"),
            Some("Detected (current)".to_string())
        );
        assert_eq!(provider.annotate("America/New_York"), None);
        assert_eq!(provider.annotate("UTC"), None);
    }

    #[test]
    fn curated_locale_names_win_over_dynamic_lookup() {
        let provider = LocaleAnnotationProvider { detected: None };

        assert_eq!(
            provider.annotate("de_DE.UTF-8"),
            Some("German (Germany)".to_string())
        );
    }

    #[test]
    fn unknown_locales_stay_unannotated_without_detection() {
        let provider = LocaleAnnotationProvider { detected: None };

        // "unknown_LO" has no curated entry and no locale definition file.
        assert_eq!(provider.annotate("unknown_LO.UTF-8"), None);
    }

    #[test]
    fn curated_keymap_names_win_over_xkb_lookup() {
        let provider = KeymapAnnotationProvider { detected: None };

        assert_eq!(provider.annotate("de-latin1"), Some("German".to_string()));
        assert_eq!(provider.annotate("uk"), Some("English (UK)".to_string()));
    }

    #[test]
    fn keymap_provider_marks_the_detected_keymap() {
        let provider = KeymapAnnotationProvider {
            detected: Some("fr".to_string()),
        };

        assert_eq!(
            provider.annotate("fr"),
            Some("French (detected)".to_string())
        );
    }

    #[test]
    fn console_keymaps_map_to_xkb_layout_names() {
        let layouts = HashMap::from([
            ("us".to_string(), "English (US)".to_string()),
            ("de".to_string(), "German".to_string()),
            ("gb".to_string(), "English (UK)".to_string()),
            ("jp".to_string(), "Japanese".to_string()),
            ("se".to_string(), "Swedish".to_string()),
            ("it".to_string(), "Italian".to_string()),
            ("ca".to_string(), "French (Canada)".to_string()),
        ]);

        // Exact code match.
        assert_eq!(console_keymap_to_xkb("us", &layouts).as_deref(), Some("us"));
        // Alias table entries.
        assert_eq!(console_keymap_to_xkb("uk", &layouts).as_deref(), Some("gb"));
        assert_eq!(
            console_keymap_to_xkb("jp106", &layouts).as_deref(),
            Some("jp")
        );
        // Variant suffixes are dropped when the prefix is a known layout.
        assert_eq!(
            console_keymap_to_xkb("de-latin1", &layouts).as_deref(),
            Some("de")
        );
        // Trailing digits are dropped when the remainder is a known layout.
        assert_eq!(
            console_keymap_to_xkb("it2", &layouts).as_deref(),
            Some("it")
        );
        // Nothing matches an unknown family.
        assert_eq!(console_keymap_to_xkb("xx-unknown", &layouts), None);
        assert_eq!(console_keymap_to_xkb("xx9", &layouts), None);
    }

    #[test]
    fn keymap_names_resolve_through_the_xkb_registry_when_available() {
        // `us` is both curated and an XKB layout; the curated entry wins and
        // the lookup must not regress either way.
        assert_eq!(keymap_display_name("us"), Some("English (US)".to_string()));
    }
}
