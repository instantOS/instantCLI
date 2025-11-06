use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;

use super::SettingsContext;

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

        match select_language_action(&state)? {
            None | Some(LanguageMenuAction::Exit) => return Ok(()),
            Some(LanguageMenuAction::SetDefault) => {
                handle_set_default(ctx, &state)?;
            }
            Some(LanguageMenuAction::EnableLocales) => {
                handle_enable_locales(ctx, &state)?;
            }
            Some(LanguageMenuAction::DisableLocales) => {
                handle_disable_locales(ctx, &state)?;
            }
        }
    }
}

fn handle_set_default(ctx: &mut SettingsContext, state: &LocaleState) -> Result<()> {
    let enabled_locales: Vec<_> = state.entries.iter().filter(|entry| entry.enabled).collect();

    if enabled_locales.is_empty() {
        ctx.emit_info(
            "settings.language.no_enabled",
            "No locales are currently generated. Enable locales first.",
        );
        return Ok(());
    }

    let current = state.current_locale.as_deref();
    let mut items: Vec<LocaleSelectionItem> = enabled_locales
        .into_iter()
        .map(|entry| LocaleSelectionItem::from_entry(entry, current))
        .collect();

    items.sort();

    let mut builder = FzfWrapper::builder()
        .prompt("System language")
        .header("Select the default system language (LANG)");

    if let Some(index) = items.iter().position(|item| item.is_current) {
        builder = builder.initial_index(index);
    }

    match builder.select(items)? {
        FzfResult::Selected(item) => {
            if Some(item.locale.as_str()) == current {
                ctx.emit_info(
                    "settings.language.unchanged",
                    "System language already set to the selected locale.",
                );
                return Ok(());
            }

            let lang_arg = format!("LANG={}", item.locale);
            ctx.run_command_as_root(
                "localectl",
                [OsStr::new("set-locale"), OsStr::new(&lang_arg)],
            )?;
            ctx.emit_success(
                "settings.language.updated",
                &format!("System language set to {}.", item.label),
            );
            ctx.notify(
                "System language",
                "Log out or reboot for all applications to pick up the new locale.",
            );
            Ok(())
        }
        FzfResult::Error(err) => bail!("fzf error: {err}"),
        _ => Ok(()),
    }
}

fn handle_enable_locales(ctx: &mut SettingsContext, state: &LocaleState) -> Result<()> {
    let disabled: Vec<_> = state
        .entries
        .iter()
        .filter(|entry| !entry.enabled)
        .collect();

    if disabled.is_empty() {
        ctx.emit_info(
            "settings.language.enable.none",
            "All available locales are already enabled.",
        );
        return Ok(());
    }

    let mut items: Vec<LocaleToggleItem> = disabled
        .into_iter()
        .map(LocaleToggleItem::from_entry)
        .collect();

    items.sort();

    let selection = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Enable locales")
        .header("Select additional locales to generate (locale-gen)")
        .select(items)?;

    let selected = match selection {
        FzfResult::MultiSelected(items) => items,
        FzfResult::Selected(item) => vec![item],
        FzfResult::Error(err) => bail!("fzf error: {err}"),
        _ => return Ok(()),
    };

    if selected.is_empty() {
        return Ok(());
    }

    let locales: Vec<String> = selected.into_iter().map(|item| item.locale).collect();
    apply_locale_gen_updates(ctx, &locales, &[])?;
    ctx.run_command_as_root("locale-gen", std::iter::empty::<&OsStr>())?;
    ctx.emit_success(
        "settings.language.enabled",
        "Selected locales enabled and locale-gen completed.",
    );
    ctx.notify(
        "Locales",
        "New locales generated. You can now set one as the system language.",
    );
    Ok(())
}

fn handle_disable_locales(ctx: &mut SettingsContext, state: &LocaleState) -> Result<()> {
    let current = state.current_locale.as_deref();
    let enabled: Vec<_> = state
        .entries
        .iter()
        .filter(|entry| entry.enabled && Some(entry.locale.as_str()) != current)
        .collect();

    if enabled.is_empty() {
        ctx.emit_info(
            "settings.language.disable.none",
            "No additional locales can be disabled.",
        );
        return Ok(());
    }

    let mut items: Vec<LocaleToggleItem> = enabled
        .into_iter()
        .map(LocaleToggleItem::from_entry)
        .collect();
    items.sort();

    let selection = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Disable locales")
        .header("Select locales to remove from /etc/locale.gen")
        .select(items)?;

    let selected = match selection {
        FzfResult::MultiSelected(items) => items,
        FzfResult::Selected(item) => vec![item],
        FzfResult::Error(err) => bail!("fzf error: {err}"),
        _ => return Ok(()),
    };

    if selected.is_empty() {
        return Ok(());
    }

    let locales: Vec<String> = selected.into_iter().map(|item| item.locale).collect();
    apply_locale_gen_updates(ctx, &[], &locales)?;
    ctx.run_command_as_root("locale-gen", std::iter::empty::<&OsStr>())?;
    ctx.emit_success(
        "settings.language.disabled",
        "Selected locales disabled and locale-gen completed.",
    );
    ctx.notify(
        "Locales",
        "Locales removed. Applications using them may need to restart.",
    );
    Ok(())
}

fn select_language_action(state: &LocaleState) -> Result<Option<LanguageMenuAction>> {
    let mut options = Vec::new();

    if !state.entries.is_empty() {
        let label = state
            .current_locale
            .as_ref()
            .and_then(|locale| state.entries.iter().find(|entry| entry.locale == *locale))
            .map(|entry| entry.label.clone())
            .unwrap_or_else(|| "Not set".to_string());
        options.push(LanguageMenuOption::new(
            LanguageMenuAction::SetDefault,
            format!("Set system language (current: {label})"),
        ));
    }

    if state.entries.iter().any(|entry| !entry.enabled) {
        options.push(LanguageMenuOption::new(
            LanguageMenuAction::EnableLocales,
            "Enable additional locales",
        ));
    }

    if state.entries.iter().filter(|entry| entry.enabled).count() > 1 {
        options.push(LanguageMenuOption::new(
            LanguageMenuAction::DisableLocales,
            "Disable locales",
        ));
    }

    options.push(LanguageMenuOption::new(LanguageMenuAction::Exit, "Back"));

    let result = FzfWrapper::builder()
        .prompt("System language")
        .header("Manage system locales")
        .select(options)?;

    Ok(match result {
        FzfResult::Selected(option) => Some(option.action),
        FzfResult::Error(err) => bail!("fzf error: {err}"),
        _ => None,
    })
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
    let mut seen_disabled = HashSet::new();
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
                seen_disabled.insert(parsed.locale.to_string());
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
    if original.ends_with('\n') {
        updated.push('\n');
    } else {
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
struct LocaleSelectionItem {
    locale: String,
    label: String,
    is_current: bool,
    has_human_name: bool,
}

impl LocaleSelectionItem {
    fn from_entry(entry: &LocaleEntry, current: Option<&str>) -> Self {
        Self {
            locale: entry.locale.clone(),
            label: entry.label.clone(),
            is_current: current == Some(entry.locale.as_str()),
            has_human_name: entry.has_human_name,
        }
    }
}

impl FzfSelectable for LocaleSelectionItem {
    fn fzf_display_text(&self) -> String {
        let marker = if self.is_current {
            format!("{} ", char::from(NerdFont::Check))
        } else {
            "   ".to_string()
        };
        format!("{marker}{}", self.label)
    }
}

impl PartialEq for LocaleSelectionItem {
    fn eq(&self, other: &Self) -> bool {
        self.locale == other.locale
    }
}

impl Eq for LocaleSelectionItem {}

impl PartialOrd for LocaleSelectionItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LocaleSelectionItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.has_human_name, other.has_human_name) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => self.label.cmp(&other.label),
        }
    }
}

#[derive(Clone)]
struct LocaleToggleItem {
    locale: String,
    label: String,
    has_human_name: bool,
}

impl LocaleToggleItem {
    fn from_entry(entry: &LocaleEntry) -> Self {
        Self {
            locale: entry.locale.clone(),
            label: entry.label.clone(),
            has_human_name: entry.has_human_name,
        }
    }
}

impl FzfSelectable for LocaleToggleItem {
    fn fzf_display_text(&self) -> String {
        self.label.to_string()
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

#[derive(Clone, Copy)]
enum LanguageMenuAction {
    SetDefault,
    EnableLocales,
    DisableLocales,
    Exit,
}

#[derive(Clone)]
struct LanguageMenuOption {
    action: LanguageMenuAction,
    label: String,
}

impl LanguageMenuOption {
    fn new(action: LanguageMenuAction, label: impl Into<String>) -> Self {
        Self {
            action,
            label: label.into(),
        }
    }
}

impl FzfSelectable for LanguageMenuOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }
}

struct LocaleState {
    entries: Vec<LocaleEntry>,
    current_locale: Option<String>,
}

impl LocaleState {
    fn load() -> Result<Self> {
        let current_locale = current_system_locale()?;

        let enabled_set = enabled_locales()?;
        let mut entries = load_available_locales(&enabled_set)?;

        if let Some(current) = &current_locale
            && !entries.iter().any(|entry| entry.locale == *current)
        {
            entries.push(LocaleEntry::fallback(current.clone(), true));
        }

        entries.sort();

        Ok(Self {
            entries,
            current_locale,
        })
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
