//! Parsing and rewriting of `/etc/locale.gen` contents.
//!
//! Pure format logic with no IO: callers read and write the file themselves
//! (the settings TUI via a privileged temp-file install, the Arch installer
//! through its command executor). See `CONTEXT.md`: an *available locale* is
//! a UTF-8 entry (offered in menus), an *enabled locale* is an uncommented
//! entry (built by `locale-gen`).

use std::collections::HashSet;

/// One parseable `/etc/locale.gen` line.
///
/// Only UTF-8 entries parse; everything else (`%` lines, blank lines, and
/// non-UTF-8 charsets) yields `None`.
struct LocaleGenLine<'a> {
    leading_ws: &'a str,
    comment_ws: &'a str,
    rest: &'a str,
    locale: &'a str,
    commented: bool,
}

impl<'a> LocaleGenLine<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        let (leading_ws, remainder) = split_leading_whitespace(line);
        if remainder.is_empty() || remainder.starts_with('%') {
            return None;
        }

        let (commented, after_comment) = if let Some(stripped) = remainder.strip_prefix('#') {
            (true, stripped)
        } else {
            (false, remainder)
        };

        let (comment_ws, content) = split_leading_whitespace(after_comment);
        if content.is_empty() || content.starts_with('%') {
            return None;
        }

        let locale_end = content
            .find(|c: char| c.is_whitespace())
            .unwrap_or(content.len());
        if locale_end == 0 {
            return None;
        }

        let locale = &content[..locale_end];
        let rest = &content[locale_end..];

        // Only process UTF-8 locales
        if !rest.trim().eq_ignore_ascii_case("UTF-8") {
            return None;
        }

        Some(Self {
            leading_ws,
            comment_ws,
            rest,
            locale,
            commented,
        })
    }

    fn with_comment(&self, commented: bool) -> String {
        let mut result = String::new();
        result.push_str(self.leading_ws);
        if commented {
            result.push('#');
        }
        result.push_str(self.comment_ws);
        result.push_str(self.locale);
        result.push_str(self.rest);
        result
    }
}

fn split_leading_whitespace(s: &str) -> (&str, &str) {
    match s.find(|c: char| !c.is_whitespace()) {
        Some(idx) => s.split_at(idx),
        None => (s, ""),
    }
}

/// Available locales (see `CONTEXT.md`): deduplicated UTF-8 entries in file order.
pub(crate) fn available_locales(contents: &str) -> Vec<String> {
    let mut locales = Vec::new();
    let mut seen = HashSet::new();

    for line in contents.lines() {
        if let Some(parsed) = LocaleGenLine::parse(line)
            && seen.insert(parsed.locale.to_string())
        {
            locales.push(parsed.locale.to_string());
        }
    }

    locales
}

/// Enabled locales (see `CONTEXT.md`): uncommented UTF-8 entries.
pub(crate) fn enabled_locales(contents: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for line in contents.lines() {
        if let Some(parsed) = LocaleGenLine::parse(line)
            && !parsed.commented
        {
            set.insert(parsed.locale.to_string());
        }
    }
    set
}

/// Enable and disable entries, preserving each line's formatting. Entries to
/// enable that are absent from the file are appended as `"{locale} UTF-8"`.
///
/// Returns `None` when nothing changed, so callers can skip writing.
pub(crate) fn apply_enable_disable(
    original: &str,
    enable: &[String],
    disable: &[String],
) -> Option<String> {
    let enable_set: HashSet<_> = enable.iter().cloned().collect();
    let disable_set: HashSet<_> = disable.iter().cloned().collect();

    let mut seen_enabled = HashSet::new();
    let mut changed = false;
    let mut new_lines = Vec::with_capacity(original.lines().count());

    for line in original.lines() {
        if let Some(parsed) = LocaleGenLine::parse(line) {
            if enable_set.contains(parsed.locale) {
                seen_enabled.insert(parsed.locale.to_string());
                if parsed.commented {
                    changed = true;
                    new_lines.push(parsed.with_comment(false));
                } else {
                    new_lines.push(line.to_string());
                }
                continue;
            }

            if disable_set.contains(parsed.locale) {
                if !parsed.commented {
                    changed = true;
                    new_lines.push(parsed.with_comment(true));
                } else {
                    new_lines.push(line.to_string());
                }
                continue;
            }
        }

        new_lines.push(line.to_string());
    }

    // Preserve the caller's order for entries that are not already present.
    // HashSet iteration order is randomized and would make repeated updates
    // produce needlessly different locale.gen files.
    for locale in enable {
        if !seen_enabled.contains(locale) {
            changed = true;
            new_lines.push(format!("{locale} UTF-8"));
            seen_enabled.insert(locale.clone());
        }
    }

    if !changed {
        return None;
    }

    let mut updated = new_lines.join("\n");
    if !updated.ends_with('\n') {
        updated.push('\n');
    }

    Some(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf8_entries_and_skips_the_rest() {
        let lines = [
            "#  en_US.UTF-8 UTF-8",
            "#  en_US ISO-8859-1",
            "de_DE.UTF-8 UTF-8",
            "de_DE ISO-8859-1",
            "",
            "   ",
            "#",
        ];

        let parsed: Vec<_> = lines
            .iter()
            .filter_map(|line| LocaleGenLine::parse(line))
            .collect();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].locale, "en_US.UTF-8");
        assert!(parsed[0].commented);
        assert_eq!(parsed[1].locale, "de_DE.UTF-8");
        assert!(!parsed[1].commented);
    }

    #[test]
    fn bare_name_utf8_entries_are_available() {
        // Pin the canonical rule: availability is decided by the charset
        // column, not by a `.UTF-8` suffix on the locale name (Q2).
        let contents = "#en_AG UTF-8\n#en_US.UTF-8 UTF-8\n#de_DE ISO-8859-1\n";

        assert_eq!(available_locales(contents), vec!["en_AG", "en_US.UTF-8"]);
    }

    #[test]
    fn available_locales_deduplicate_in_file_order() {
        let contents = "#en_US.UTF-8 UTF-8\n#de_DE.UTF-8 UTF-8\nen_US.UTF-8 UTF-8\n";

        assert_eq!(
            available_locales(contents),
            vec!["en_US.UTF-8", "de_DE.UTF-8"]
        );
    }

    #[test]
    fn apply_enables_disables_and_appends() {
        let original = "#  en_US.UTF-8 UTF-8\nde_DE.UTF-8 UTF-8\n#fr_FR.UTF-8 UTF-8\n";

        let updated = apply_enable_disable(
            original,
            &["en_US.UTF-8".to_string(), "nl_NL.UTF-8".to_string()],
            &["de_DE.UTF-8".to_string()],
        )
        .expect("changes expected");

        assert_eq!(
            updated,
            "  en_US.UTF-8 UTF-8\n#de_DE.UTF-8 UTF-8\n#fr_FR.UTF-8 UTF-8\nnl_NL.UTF-8 UTF-8\n"
        );
    }

    #[test]
    fn appends_missing_locales_in_requested_order_without_duplicates() {
        let updated = apply_enable_disable(
            "#en_US.UTF-8 UTF-8\n",
            &[
                "fr_FR.UTF-8".to_string(),
                "de_DE.UTF-8".to_string(),
                "fr_FR.UTF-8".to_string(),
            ],
            &[],
        )
        .expect("changes expected");

        assert_eq!(
            updated,
            "#en_US.UTF-8 UTF-8\nfr_FR.UTF-8 UTF-8\nde_DE.UTF-8 UTF-8\n"
        );
    }

    #[test]
    fn apply_returns_none_when_nothing_changes() {
        let original = "en_US.UTF-8 UTF-8\n#de_DE ISO-8859-1\n";

        assert_eq!(
            apply_enable_disable(original, &["en_US.UTF-8".to_string()], &[]),
            None
        );
    }
}
