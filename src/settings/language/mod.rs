use std::ffi::OsStr;
use std::iter;

use anyhow::{Result, bail};

use crate::menu_utils::{FzfResult, FzfWrapper};

use super::SettingsContext;
use crate::menu_utils::select_one_with_style;

mod locale_gen;
mod menu;
mod state;

use locale_gen::apply_locale_gen_updates;
use menu::{LanguageMenuItem, LocaleActionItem, LocaleToggleItem, build_language_menu_items};
use state::LocaleState;

pub fn configure_system_language(ctx: &mut SettingsContext) -> Result<()> {
    // Check for systemd availability (localectl)
    if which::which("localectl").is_err() {
        ctx.emit_unsupported(
            "settings.language.no_systemd",
            "Language configuration requires systemd (localectl not found).",
        );
        return Ok(());
    }

    loop {
        let state = LocaleState::load()?;

        if state.entries().is_empty() {
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

    let is_current = state.current_locale() == Some(entry.locale.as_str());
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
        .entries()
        .iter()
        .filter(|entry| !entry.enabled)
        .cloned()
        .map(|entry| LocaleToggleItem::from_entry(entry, state.current_locale()))
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
        &format!("Language set to {label}."),
    );
    ctx.notify(
        "Language",
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
