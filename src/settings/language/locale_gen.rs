use std::collections::HashSet;
use std::fs;
use std::io::Write;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use super::SettingsContext;

pub(super) const LOCALE_GEN_PATH: &str = "/etc/locale.gen";

pub(super) fn enabled_locales() -> Result<HashSet<String>> {
    let contents = fs::read_to_string(LOCALE_GEN_PATH)
        .with_context(|| format!("reading {LOCALE_GEN_PATH}"))?;

    let mut set = HashSet::new();
    for line in contents.lines() {
        if let Some(parsed) = LocaleGenLine::parse(line)
            && !parsed.commented
        {
            set.insert(parsed.locale.to_string());
        }
    }
    Ok(set)
}

pub(super) fn all_locale_gen_entries() -> Result<Vec<String>> {
    let contents = fs::read_to_string(LOCALE_GEN_PATH)
        .with_context(|| format!("reading {LOCALE_GEN_PATH}"))?;

    let mut locales = Vec::new();
    let mut seen = HashSet::new();

    for line in contents.lines() {
        if let Some(parsed) = LocaleGenLine::parse(line) {
            // Only show UTF-8 locales in the menu
            if !parsed.rest.trim().eq_ignore_ascii_case("UTF-8") {
                continue;
            }

            let locale = parsed.locale.to_string();
            if seen.insert(locale.clone()) {
                locales.push(locale);
            }
        }
    }

    Ok(locales)
}

pub(super) fn apply_locale_gen_updates(
    ctx: &mut SettingsContext,
    enable: &[String],
    disable: &[String],
) -> Result<()> {
    let original = fs::read_to_string(LOCALE_GEN_PATH)
        .with_context(|| format!("reading {LOCALE_GEN_PATH}"))?;

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

    for locale in enable_set {
        if !seen_enabled.contains(&locale) {
            changed = true;
            new_lines.push(format!("{locale} UTF-8"));
        }
    }

    if !changed {
        return Ok(());
    }

    let mut updated = new_lines.join("\n");
    if !updated.ends_with('\n') {
        updated.push('\n');
    }

    write_locale_gen(ctx, &updated)?;
    Ok(())
}

fn write_locale_gen(ctx: &mut SettingsContext, contents: &str) -> Result<()> {
    let mut temp = NamedTempFile::new().context("creating temporary locale.gen")?;
    temp.write_all(contents.as_bytes())
        .context("writing temporary locale.gen")?;
    temp.flush().context("flushing temporary locale.gen")?;
    let temp_path = temp.into_temp_path();

    ctx.run_command_as_root(
        "install",
        [
            std::ffi::OsStr::new("-m"),
            std::ffi::OsStr::new("644"),
            temp_path.as_os_str(),
            std::ffi::OsStr::new(LOCALE_GEN_PATH),
        ],
    )?;

    temp_path.close().context("removing temporary locale.gen")?;
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_locale_lines() {
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

        // Check that we can distinguish UTF-8 from others
        assert_eq!(parsed[0].locale, "en_US.UTF-8");
        assert!(parsed[0].rest.trim().eq_ignore_ascii_case("UTF-8"));

        assert_eq!(parsed[1].locale, "de_DE.UTF-8");
        assert!(parsed[1].rest.trim().eq_ignore_ascii_case("UTF-8"));
    }
}
