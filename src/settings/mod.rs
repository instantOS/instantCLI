mod registry;
mod store;

use std::process::Command;

use anyhow::{Context, Result};
use duct::cmd;

use crate::fzf_wrapper::{ConfirmResult, FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;
pub use store::{BoolSettingKey, SettingsStore, StringSettingKey};

use registry::{CATEGORIES, SettingCategory, SettingDefinition, SettingKind, SettingOption};

#[derive(Debug)]
pub struct SettingsContext {
    store: SettingsStore,
    dirty: bool,
    debug: bool,
}

impl SettingsContext {
    pub fn new(store: SettingsStore, debug: bool) -> Self {
        Self {
            store,
            dirty: false,
            debug,
        }
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn bool(&self, key: BoolSettingKey) -> bool {
        self.store.bool(key)
    }

    pub fn set_bool(&mut self, key: BoolSettingKey, value: bool) {
        if self.store.bool(key) != value {
            self.store.set_bool(key, value);
            self.dirty = true;
        }
    }

    pub fn string(&self, key: StringSettingKey) -> String {
        self.store.string(key)
    }

    pub fn set_string(&mut self, key: StringSettingKey, value: &str) {
        if self.store.string(key) != value {
            self.store.set_string(key, value);
            self.dirty = true;
        }
    }

    pub fn persist(&mut self) -> Result<()> {
        if self.dirty {
            self.store.save()?;
            self.dirty = false;
        }
        Ok(())
    }

    pub fn notify(&self, summary: &str, body: &str) {
        if self.debug {
            let message = format!(
                "{} {}",
                char::from(Fa::InfoCircle),
                format!("{summary}: {body}")
            );
            emit(Level::Debug, "settings.notify", &message, None);
            return;
        }

        let result = cmd!("notify-send", summary, body).run();
        if let Err(err) = result {
            let message = format!(
                "{} Failed to send notification: {err}",
                char::from(Fa::ExclamationCircle)
            );
            emit(Level::Debug, "settings.notify.error", &message, None);
        }
    }

    pub fn emit_success(&self, code: &str, message: &str) {
        emit(
            Level::Success,
            code,
            &format!("{} {message}", char::from(Fa::Check)),
            None,
        );
    }

    pub fn emit_info(&self, code: &str, message: &str) {
        emit(
            Level::Info,
            code,
            &format!("{} {message}", char::from(Fa::InfoCircle)),
            None,
        );
    }
}

pub fn handle_settings_command(debug: bool) -> Result<()> {
    let store = SettingsStore::load().context("loading settings file")?;
    let mut ctx = SettingsContext::new(store, debug);

    loop {
        let category_items: Vec<CategoryItem> = CATEGORIES
            .iter()
            .map(|category| CategoryItem {
                category,
                total: registry::settings_for_category(category.id).len(),
            })
            .collect();

        if category_items.is_empty() {
            emit(
                Level::Warn,
                "settings.empty",
                &format!(
                    "{} No settings registered yet.",
                    char::from(Fa::ExclamationCircle)
                ),
                None,
            );
            break;
        }

        match FzfWrapper::select_one(category_items)? {
            Some(item) => {
                if !handle_category(&mut ctx, item.category)? {
                    break;
                }
            }
            None => break,
        }
    }

    ctx.persist()?;
    Ok(())
}

fn handle_category(ctx: &mut SettingsContext, category: &'static SettingCategory) -> Result<bool> {
    let setting_defs = registry::settings_for_category(category.id);
    if setting_defs.is_empty() {
        ctx.emit_info(
            "settings.category.empty",
            &format!("No settings available for {} yet.", category.title),
        );
        return Ok(true);
    }

    let mut items: Vec<SettingItem> = Vec::new();
    for definition in setting_defs {
        let state = match &definition.kind {
            SettingKind::Toggle { key, .. } => SettingState::Toggle {
                enabled: ctx.bool(*key),
            },
            SettingKind::Choice { key, options, .. } => {
                let current_value = ctx.string(*key);
                let current_index = options
                    .iter()
                    .position(|option| option.value == current_value);
                SettingState::Choice { current_index }
            }
            SettingKind::Action { .. } => SettingState::Action,
        };

        items.push(SettingItem { definition, state });
    }

    match FzfWrapper::select_one(items)? {
        Some(item) => {
            handle_setting(ctx, item.definition, item.state)?;
            ctx.persist()?;
            Ok(true)
        }
        None => Ok(true),
    }
}

fn handle_setting(
    ctx: &mut SettingsContext,
    definition: &'static SettingDefinition,
    state: SettingState,
) -> Result<()> {
    match &definition.kind {
        SettingKind::Toggle {
            key,
            summary,
            apply,
        } => {
            let current = matches!(state, SettingState::Toggle { enabled: true });
            let next = !current;
            let intent = if next { "Enable" } else { "Disable" };
            let prompt = format!(
                "{} {} {}?\n\n{}",
                char::from(if next { Fa::ToggleOn } else { Fa::ToggleOff }),
                intent,
                definition.title,
                summary
            );

            match FzfWrapper::confirm(&prompt)? {
                ConfirmResult::Yes => {
                    ctx.set_bool(*key, next);
                    if let Some(apply_fn) = apply {
                        apply_fn(ctx, next)?;
                    }
                    ctx.emit_success(
                        "settings.toggle.updated",
                        &format!(
                            "{} {}",
                            definition.title,
                            if next { "enabled" } else { "disabled" }
                        ),
                    );
                }
                _ => {
                    ctx.emit_info("settings.toggle.cancelled", "No changes made.");
                }
            }
        }
        SettingKind::Choice {
            key,
            options,
            summary,
            apply,
        } => {
            let items: Vec<ChoiceItem> = options
                .iter()
                .enumerate()
                .map(|(index, option)| ChoiceItem {
                    option,
                    is_current: matches!(
                        state,
                        SettingState::Choice {
                            current_index: Some(current)
                        } if current == index
                    ),
                    summary,
                })
                .collect();

            match FzfWrapper::select_one(items)? {
                Some(choice) => {
                    ctx.set_string(*key, choice.option.value);
                    if let Some(apply_fn) = apply {
                        apply_fn(ctx, choice.option)?;
                    }
                    ctx.emit_success(
                        "settings.choice.updated",
                        &format!("{} set to {}", definition.title, choice.option.label),
                    );
                }
                None => ctx.emit_info("settings.choice.cancelled", "No changes made."),
            }
        }
        SettingKind::Action { summary, run } => {
            ctx.emit_info("settings.action.running", &format!("{}", summary));
            run(ctx)?;
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct CategoryItem {
    category: &'static SettingCategory,
    total: usize,
}

#[derive(Clone, Copy)]
struct SettingItem {
    definition: &'static SettingDefinition,
    state: SettingState,
}

#[derive(Clone, Copy)]
enum SettingState {
    Toggle { enabled: bool },
    Choice { current_index: Option<usize> },
    Action,
}

#[derive(Clone, Copy)]
struct ChoiceItem {
    option: &'static SettingOption,
    is_current: bool,
    summary: &'static str,
}

impl FzfSelectable for CategoryItem {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} {} ({} settings)",
            char::from(self.category.icon),
            self.category.title,
            self.total
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(self.category.description.to_string())
    }
}

impl FzfSelectable for SettingItem {
    fn fzf_display_text(&self) -> String {
        match self.state {
            SettingState::Toggle { enabled } => {
                let glyph = if enabled { Fa::ToggleOn } else { Fa::ToggleOff };
                format!("{} {}", char::from(glyph), self.definition.title)
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
                    char::from(glyph),
                    self.definition.title,
                    current_label
                )
            }
            SettingState::Action => format!(
                "{} {}",
                char::from(self.definition.icon),
                self.definition.title
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match &self.definition.kind {
            SettingKind::Toggle { summary, .. }
            | SettingKind::Choice { summary, .. }
            | SettingKind::Action { summary, .. } => FzfPreview::Text(summary.to_string()),
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
        format!("{} {}", char::from(glyph), self.option.label)
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(format!("{}\n\n{}", self.option.description, self.summary))
    }
}

pub(super) fn apply_clipboard_manager(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let is_running = Command::new("pgrep")
        .arg("-f")
        .arg("clipmenud")
        .output()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false);

    if enabled && !is_running {
        if let Err(err) = Command::new("clipmenud").spawn() {
            emit(
                Level::Warn,
                "settings.clipboard.spawn_failed",
                &format!(
                    "{} Failed to launch clipmenud: {err}",
                    char::from(Fa::ExclamationCircle)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud started");
        }
    } else if !enabled && is_running {
        if let Err(err) = cmd!("pkill", "-f", "clipmenud").run() {
            emit(
                Level::Warn,
                "settings.clipboard.stop_failed",
                &format!(
                    "{} Failed to stop clipmenud: {err}",
                    char::from(Fa::ExclamationCircle)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud stopped");
        }
    }

    Ok(())
}
