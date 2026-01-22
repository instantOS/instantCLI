use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::locale_gen;

const LOCALE_DATA_DIR: &str = "/usr/share/i18n/locales";

#[derive(Clone)]
pub(super) struct LocaleState {
    entries: Vec<LocaleEntry>,
    current_locale: Option<String>,
}

impl LocaleState {
    pub(super) fn load() -> Result<Self> {
        let current_locale = current_system_locale()?;
        let enabled = locale_gen::enabled_locales()?;
        let mut entries = load_available_locales(&enabled)?;

        if let Some(current) = &current_locale
            && !entries.iter().any(|entry| entry.locale == *current)
        {
            let is_enabled = enabled.contains(current);
            entries.push(LocaleEntry::fallback(current.clone(), is_enabled));
        }

        entries.sort();

        Ok(Self {
            entries,
            current_locale,
        })
    }

    pub(super) fn entries(&self) -> &[LocaleEntry] {
        &self.entries
    }

    pub(super) fn current_locale(&self) -> Option<&str> {
        self.current_locale.as_deref()
    }

    pub(super) fn entry(&self, locale: &str) -> Option<&LocaleEntry> {
        self.entries.iter().find(|entry| entry.locale == locale)
    }

    pub(super) fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.enabled).count()
    }
}

#[derive(Clone)]
pub(super) struct LocaleEntry {
    pub(super) locale: String,
    pub(super) label: String,
    pub(super) has_human_name: bool,
    pub(super) enabled: bool,
}

impl LocaleEntry {
    fn new(locale: String, metadata: Option<LocaleMetadata>, enabled: bool) -> Self {
        let (label, has_human_name) = match metadata.and_then(|meta| meta.display_name) {
            Some(name) => (format!("{name} ({locale})"), true),
            None => (locale.clone(), false),
        };

        Self {
            locale,
            label,
            has_human_name,
            enabled,
        }
    }

    fn fallback(locale: String, enabled: bool) -> Self {
        Self {
            locale: locale.clone(),
            label: locale,
            has_human_name: false,
            enabled,
        }
    }
}

impl PartialEq for LocaleEntry {
    fn eq(&self, other: &Self) -> bool {
        self.locale == other.locale
    }
}

impl Eq for LocaleEntry {}

impl PartialOrd for LocaleEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LocaleEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.has_human_name, other.has_human_name) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => self.label.cmp(&other.label),
        }
    }
}

#[derive(Clone)]
struct LocaleMetadata {
    display_name: Option<String>,
}

fn load_available_locales(enabled: &HashSet<String>) -> Result<Vec<LocaleEntry>> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(mut locales) = read_command_lines({
        let mut command = Command::new("localectl");
        command.arg("list-locales");
        command
    }) {
        locales.retain(|locale| locale.contains("UTF-8") || enabled.contains(locale));

        for locale in locales {
            if seen.insert(locale.clone()) {
                let metadata = locale_metadata(&locale)?;
                let is_enabled = enabled.contains(&locale);
                entries.push(LocaleEntry::new(locale, metadata, is_enabled));
            }
        }
    }

    for locale in locale_gen::all_locale_gen_entries()? {
        if seen.insert(locale.clone()) {
            let metadata = locale_metadata(&locale)?;
            let is_enabled = enabled.contains(&locale);
            entries.push(LocaleEntry::new(locale, metadata, is_enabled));
        }
    }

    for locale in enabled {
        if seen.insert(locale.clone()) {
            entries.push(LocaleEntry::fallback(locale.clone(), true));
        }
    }

    Ok(entries)
}

fn locale_metadata(locale: &str) -> Result<Option<LocaleMetadata>> {
    let base = locale_base(locale);
    let path = Path::new(LOCALE_DATA_DIR).join(base);
    if !path.exists() {
        return Ok(None);
    }

    let metadata = parse_locale_file(&path)?;
    Ok(metadata)
}

fn parse_locale_file(path: &Path) -> Result<Option<LocaleMetadata>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("reading locale data from {}", path.display()))?;

    let mut in_identification = false;
    let mut title = None;
    let mut language = None;
    let mut territory = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("LC_IDENTIFICATION") {
            in_identification = true;
            continue;
        }

        if trimmed.starts_with("END LC_IDENTIFICATION") {
            break;
        }

        if !in_identification {
            continue;
        }

        if trimmed.starts_with("title") {
            title = parse_quoted_value(trimmed);
        } else if trimmed.starts_with("language") {
            language = parse_quoted_value(trimmed);
        } else if trimmed.starts_with("territory") {
            territory = parse_quoted_value(trimmed);
        }
    }

    let display_name = if let Some(title) = title {
        Some(title)
    } else if let Some(lang) = language {
        match territory {
            Some(country) if !country.is_empty() => Some(format!("{lang} ({country})")),
            _ => Some(lang),
        }
    } else {
        None
    };

    Ok(Some(LocaleMetadata { display_name }))
}

fn parse_quoted_value(input: &str) -> Option<String> {
    let start = input.find('"')?;
    let rest = &input[start + 1..];
    let end = rest.find('"')?;
    let value = &rest[..end];
    Some(value.replace("\\\"", "\""))
}

fn locale_base(locale: &str) -> &str {
    let mut base = locale;
    if let Some(idx) = base.find('.') {
        base = &base[..idx];
    }
    if let Some(idx) = base.find('@') {
        base = &base[..idx];
    }
    base
}

fn current_system_locale() -> Result<Option<String>> {
    let output = match Command::new("localectl")
        .arg("status")
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(None), // localectl not available
    };

    if !output.status.success() {
        return Ok(None);
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("System Locale:") {
            for part in rest.split_whitespace() {
                if let Some(lang) = part.strip_prefix("LANG=") {
                    return Ok(Some(lang.to_string()));
                }
            }
        }
    }

    Ok(None)
}

fn read_command_lines(mut command: Command) -> Result<Vec<String>> {
    let program = command.get_program().to_owned();
    let output = command
        .output()
        .with_context(|| format!("running {:?}", program))?;

    if !output.status.success() {
        bail!(
            "command {:?} exited with status {:?}",
            program,
            output.status.code()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}
