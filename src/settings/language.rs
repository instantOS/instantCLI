use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::iter;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::SettingsContext;
use super::context::{format_icon, select_one_with_style};

const LOCALE_GEN_PATH: &str = "/etc/locale.gen";
const LOCALE_DATA_DIR: &str = "/usr/share/i18n/locales";

pub fn configure_system_language(ctx: &mut SettingsContext) -> Result<()> {
    loop {
        let state = LocaleState::load()?;

        if state.entries.is_empty() {
            ctx.emit_info(
                "settings.language.none",
                "No locales reported by localectl. Ensure glibc locale data is installed.",
            );
            return Ok(());
        }

        let menu_items = build_language_menu_items(&state);

        match select_one_with_style(menu_items)? {
            Some(LanguageMenuItem::Locale(locale_item)) => {
                if handle_locale_entry(ctx, &state, locale_item.locale.clone())? {
                    continue;
                }
            }
            Some(LanguageMenuItem::Add) => {
                if handle_add_locale(ctx, &state)? {
                    continue;
                }
            }
            _ => return Ok(()),
        }
    }
}

fn build_language_menu_items(state: &LocaleState) -> Vec<LanguageMenuItem> {
    let mut entries: Vec<LocaleMenuEntry> = state
        .entries
        .iter()
        .filter(|&entry| {
            entry.enabled
                || state
                    .current_locale
                    .as_deref()
                    .map(|locale| locale == entry.locale)
                    .unwrap_or(false)
        })
        .cloned()
        .map(|entry| LocaleMenuEntry::from_entry(entry, state.current_locale.as_deref()))
        .collect();

    entries.sort();

    let mut items: Vec<LanguageMenuItem> =
        entries.into_iter().map(LanguageMenuItem::Locale).collect();

    items.push(LanguageMenuItem::Add);
    items.push(LanguageMenuItem::Back);

    items
}

fn handle_locale_entry(
    ctx: &mut SettingsContext,
    state: &LocaleState,
    locale: String,
) -> Result<bool> {
    let entry = match state.entry(&locale) {
        Some(entry) => entry.clone(),
        None => {
            ctx.emit_info(
                "settings.language.missing",
                "Locale no longer available. Refreshing list.",
            );
            return Ok(true);
        }
    };

    let is_current = state.current_locale.as_deref() == Some(entry.locale.as_str());
    let multiple_enabled = state.enabled_count() > 1;
    let can_remove = entry.enabled && multiple_enabled && !is_current;

    let mut actions = Vec::new();

    if !is_current {
        actions.push(LocaleActionItem::SetDefault {
            locale: entry.locale.clone(),
            label: entry.label.clone(),
        });
    }

    if can_remove {
        actions.push(LocaleActionItem::Remove {
            locale: entry.locale.clone(),
            label: entry.label.clone(),
        });
    }

    actions.push(LocaleActionItem::Back);

    match select_one_with_style(actions)? {
        Some(LocaleActionItem::SetDefault { locale, label }) => {
            set_system_language(ctx, &locale, &label)?;
            Ok(true)
        }
        Some(LocaleActionItem::Remove { locale, label }) => {
            disable_locale(ctx, &locale, &label)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_add_locale(ctx: &mut SettingsContext, state: &LocaleState) -> Result<bool> {
    let mut candidates: Vec<LocaleToggleItem> = state
        .entries
        .iter()
        .filter(|entry| !entry.enabled)
        .cloned()
        .map(LocaleToggleItem::from_entry)
        .collect();

    if candidates.is_empty() {
        ctx.emit_info(
            "settings.language.add.none",
            "All locales reported by localectl are already enabled.",
        );
        return Ok(false);
    }

    candidates.sort();

    let selection = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Add locale")
        .header("Select locales to enable (locale-gen)")
        .select(candidates)?;

    let selected = match selection {
        FzfResult::MultiSelected(items) => items,
        FzfResult::Selected(item) => vec![item],
        FzfResult::Error(err) => bail!("fzf error: {err}"),
        _ => return Ok(false),
    };

    if selected.is_empty() {
        return Ok(false);
    }

    let locales: Vec<String> = selected.into_iter().map(|item| item.locale).collect();
    enable_locales(ctx, &locales)?;
    Ok(true)
}

fn set_system_language(ctx: &mut SettingsContext, locale: &str, label: &str) -> Result<()> {
    let lang_arg = format!("LANG={locale}");
    ctx.run_command_as_root(
        "localectl",
        [OsStr::new("set-locale"), OsStr::new(&lang_arg)],
    )?;
    ctx.emit_success(
        "settings.language.updated",
        &format!("System language set to {label}."),
    );
    ctx.notify(
        "System language",
        "Log out or reboot for applications to use the new locale.",
    );
    Ok(())
}

fn enable_locales(ctx: &mut SettingsContext, locales: &[String]) -> Result<()> {
    apply_locale_gen_updates(ctx, locales, &[])?;
    ctx.run_command_as_root("locale-gen", iter::empty::<&OsStr>())?;
    ctx.emit_success(
        "settings.language.enabled",
        "Selected locales were generated successfully.",
    );
    ctx.notify(
        "Locales",
        "Locales are ready. Set one as the default language if desired.",
    );
    Ok(())
}

fn disable_locale(ctx: &mut SettingsContext, locale: &str, label: &str) -> Result<()> {
    apply_locale_gen_updates(ctx, &[], &[locale.to_string()])?;
    ctx.run_command_as_root("locale-gen", iter::empty::<&OsStr>())?;
    ctx.emit_success(
        "settings.language.disabled",
        &format!("{label} removed from generated locales."),
    );
    ctx.notify(
        "Locales",
        "Locale removed. Restart applications that used it.",
    );
    Ok(())
}

fn apply_locale_gen_updates(
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
            OsStr::new("-m"),
            OsStr::new("644"),
            temp_path.as_os_str(),
            OsStr::new(LOCALE_GEN_PATH),
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

        let (commented, after_comment) = if remainder.starts_with('#') {
            (true, &remainder[1..])
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

#[derive(Clone)]
enum LanguageMenuItem {
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
struct LocaleMenuEntry {
    locale: String,
    label: String,
    is_current: bool,
    has_human_name: bool,
    enabled: bool,
}

impl LocaleMenuEntry {
    fn from_entry(entry: LocaleEntry, current: Option<&str>) -> Self {
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
        } else if self.enabled {
            format!("{} ", char::from(NerdFont::CircleCheck))
        } else {
            format!("{} ", char::from(NerdFont::Circle))
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
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LocaleMenuEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.has_human_name, other.has_human_name) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => self.label.cmp(&other.label),
        }
    }
}

#[derive(Clone)]
enum LocaleActionItem {
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
struct LocaleToggleItem {
    locale: String,
    label: String,
    has_human_name: bool,
}

impl LocaleToggleItem {
    fn from_entry(entry: LocaleEntry) -> Self {
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
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LocaleToggleItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.has_human_name, other.has_human_name) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => self.label.cmp(&other.label),
        }
    }
}

struct LocaleState {
    entries: Vec<LocaleEntry>,
    current_locale: Option<String>,
}

impl LocaleState {
    fn load() -> Result<Self> {
        let current_locale = current_system_locale()?;
        let enabled = enabled_locales()?;
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

    fn entry(&self, locale: &str) -> Option<&LocaleEntry> {
        self.entries.iter().find(|entry| entry.locale == locale)
    }

    fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.enabled).count()
    }
}

#[derive(Clone)]
struct LocaleEntry {
    locale: String,
    label: String,
    has_human_name: bool,
    enabled: bool,
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
    let mut locales = read_command_lines({
        let mut command = Command::new("localectl");
        command.arg("list-locales");
        command
    })?;

    // Keep UTF-8 locales by default for better compatibility
    locales.retain(|locale| locale.contains("UTF-8") || enabled.contains(locale));

    let mut entries = Vec::with_capacity(locales.len());
    let mut seen = HashSet::new();

    for locale in locales {
        if seen.insert(locale.clone()) {
            let metadata = locale_metadata(&locale)?;
            let enabled = enabled.contains(&locale);
            entries.push(LocaleEntry::new(locale, metadata, enabled));
        }
    }

    for locale in enabled {
        if seen.insert(locale.clone()) {
            entries.push(LocaleEntry::fallback(locale.clone(), true));
        }
    }

    Ok(entries)
}

fn enabled_locales() -> Result<HashSet<String>> {
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
    let output = Command::new("localectl")
        .arg("status")
        .output()
        .context("running localectl status")?;

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

fn split_leading_whitespace(s: &str) -> (&str, &str) {
    match s.find(|c: char| !c.is_whitespace()) {
        Some(idx) => s.split_at(idx),
        None => (s, ""),
    }
}
