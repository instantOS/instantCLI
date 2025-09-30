mod registry;
mod store;
mod users;

use std::{env, path::PathBuf, process::Command};

use anyhow::{bail, Context, Result};
use clap::{Subcommand, ValueHint};
use duct::cmd;
use sudo::RunningAs;

use crate::common::requirements::RequiredPackage;
use crate::fzf_wrapper::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;
pub use store::{BoolSettingKey, SettingsStore, StringSettingKey};

use registry::{
    CATEGORIES, CommandSpec, CommandStyle, SettingCategory, SettingDefinition, SettingKind,
    SettingOption,
};

#[derive(Subcommand, Debug, Clone)]
pub enum SettingsCommands {
    /// Reapply settings that do not persist across reboots
    Apply,
    #[command(hide = true)]
    InternalApply {
        #[arg(long = "setting-id")]
        setting_id: String,
        #[arg(long = "bool-value")]
        bool_value: Option<bool>,
        #[arg(long = "string-value")]
        string_value: Option<String>,
        #[arg(long = "settings-file", value_hint = ValueHint::FilePath)]
        settings_file: Option<PathBuf>,
    },
}

#[derive(Debug)]
pub struct SettingsContext {
    store: SettingsStore,
    dirty: bool,
    debug: bool,
    privileged: bool,
    current_definition: Option<&'static SettingDefinition>,
}

impl SettingsContext {
    pub fn new(store: SettingsStore, debug: bool, privileged_flag: bool) -> Self {
        Self {
            store,
            dirty: false,
            debug,
            privileged: privileged_flag || matches!(sudo::check(), RunningAs::Root),
            current_definition: None,
        }
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn is_privileged(&self) -> bool {
        self.privileged
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

    pub fn with_definition<F>(&mut self, definition: &'static SettingDefinition, f: F) -> Result<()>
    where
        F: FnOnce(&mut SettingsContext) -> Result<()>,
    {
        let previous = self.current_definition;
        self.current_definition = Some(definition);
        let result = f(self);
        self.current_definition = previous;
        result
    }

    pub fn request_privileged_bool(&mut self, value: bool) -> Result<()> {
        self.invoke_privileged(PrivilegedValue::Bool(value))
    }

    pub fn request_privileged_choice<S: Into<String>>(&mut self, value: S) -> Result<()> {
        self.invoke_privileged(PrivilegedValue::Choice(value.into()))
    }

    fn invoke_privileged(&mut self, value: PrivilegedValue) -> Result<()> {
        if self.privileged {
            return Ok(());
        }

        let definition = self
            .current_definition
            .ok_or_else(|| anyhow::anyhow!("no active setting definition for privilege escalation"))?;

        let exe = env::current_exe().context("locating executable")?;
        let settings_path = self.store.path().to_path_buf();

        let mut command = Command::new("/usr/bin/sudo");
        command.arg(&exe);
        if self.debug {
            command.arg("--debug");
        }
        command.arg("--internal-privileged-mode");
        command.arg("settings");
        command.arg("internal-apply");
        command.arg("--setting-id");
        command.arg(definition.id);
        command.arg("--settings-file");
        command.arg(&settings_path);
        match value {
            PrivilegedValue::Bool(v) => {
                command.arg("--bool-value");
                command.arg(if v { "true" } else { "false" });
            }
            PrivilegedValue::Choice(v) => {
                command.arg("--string-value");
                command.arg(v);
            }
        }

        let status = command
            .status()
            .with_context(|| format!("escalating setting {}", definition.id))?;

        if !status.success() {
            bail!("privileged apply for {} exited with status {:?}", definition.id, status.code());
        }

        Ok(())
    }

    pub fn run_command_as_root<I, S>(&self, program: S, args: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: AsRef<std::ffi::OsStr>,
        S: AsRef<std::ffi::OsStr>,
    {
        let status = if self.privileged {
            let mut command = Command::new(program);
            command.args(args);
            command.status()
        } else {
            let mut command = Command::new("/usr/bin/sudo");
            command.arg(program);
            command.args(args);
            command.status()
        }?;

        if !status.success() {
            bail!("command {:?} failed with status {:?}", program.as_ref(), status.code());
        }

        Ok(())
    }
}

enum PrivilegedValue {
    Bool(bool),
    Choice(String),
}

enum ApplyOverride {
    Bool(bool),
    Choice(&'static SettingOption),
}

fn apply_definition(
    ctx: &mut SettingsContext,
    definition: &'static SettingDefinition,
    override_value: Option<ApplyOverride>,
) -> Result<()> {
    match (&definition.kind, override_value) {
        (
            SettingKind::Toggle {
                key,
                apply: Some(apply_fn),
                ..
            },
            Some(ApplyOverride::Bool(value)),
        ) => ctx.with_definition(definition, |ctx| apply_fn(ctx, value)),
        (
            SettingKind::Toggle {
                key,
                apply: Some(apply_fn),
                ..
            },
            None,
        ) => {
            let value = ctx.bool(*key);
            ctx.with_definition(definition, |ctx| apply_fn(ctx, value))
        }
        (
            SettingKind::Choice {
                key,
                options,
                apply: Some(apply_fn),
                ..
            },
            Some(ApplyOverride::Choice(option)),
        ) => ctx.with_definition(definition, |ctx| apply_fn(ctx, option)),
        (
            SettingKind::Choice {
                key,
                options,
                apply: Some(apply_fn),
                ..
            },
            None,
        ) => {
            let current_value = ctx.string(*key);
            if let Some(option) = options.iter().find(|candidate| candidate.value == current_value)
            {
                ctx.with_definition(definition, |ctx| apply_fn(ctx, option))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

pub fn dispatch_settings_command(
    debug: bool,
    privileged_flag: bool,
    command: Option<SettingsCommands>,
) -> Result<()> {
    match command {
        None => run_settings_ui(debug, privileged_flag),
        Some(SettingsCommands::Apply) => run_nonpersistent_apply(debug, privileged_flag),
        Some(SettingsCommands::InternalApply {
            setting_id,
            bool_value,
            string_value,
            settings_file,
        }) => run_internal_apply(
            debug,
            privileged_flag,
            &setting_id,
            bool_value,
            string_value,
            settings_file,
        ),
    }
}

fn run_settings_ui(debug: bool, privileged_flag: bool) -> Result<()> {
    let store = SettingsStore::load().context("loading settings file")?;
    let mut ctx = SettingsContext::new(store, debug, privileged_flag);

    loop {
        let mut menu_items = Vec::with_capacity(CATEGORIES.len() + 1);
        menu_items.push(CategoryMenuItem::SearchAll);

        let mut total_settings = 0usize;
        for category in CATEGORIES {
            let definitions = registry::settings_for_category(category.id);
            total_settings += definitions.len();

            let mut toggles = 0usize;
            let mut choices = 0usize;
            let mut actions = 0usize;
            let mut commands = 0usize;
            let mut highlights = [None, None, None];

            for (idx, definition) in definitions.iter().enumerate() {
                match definition.kind {
                    SettingKind::Toggle { .. } => toggles += 1,
                    SettingKind::Choice { .. } => choices += 1,
                    SettingKind::Action { .. } => actions += 1,
                    SettingKind::Command { .. } => commands += 1,
                }

                if idx < highlights.len() {
                    highlights[idx] = Some(*definition);
                }
            }

            menu_items.push(CategoryMenuItem::Category(CategoryItem {
                category,
                total: definitions.len(),
                toggles,
                choices,
                actions,
                commands,
                highlights,
            }));
        }

        if total_settings == 0 {
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

        match FzfWrapper::select_one(menu_items)? {
            Some(CategoryMenuItem::SearchAll) => {
                if !handle_search_all(&mut ctx)? {
                    break;
                }
            }
            Some(CategoryMenuItem::Category(item)) => {
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

fn run_nonpersistent_apply(debug: bool, privileged_flag: bool) -> Result<()> {
    let store = SettingsStore::load().context("loading settings file")?;
    let mut ctx = SettingsContext::new(store, debug, privileged_flag);

    let mut applied = 0usize;

    for definition in SETTINGS.iter().filter(|definition| definition.requires_reapply) {
        match definition.kind {
            SettingKind::Toggle { .. } | SettingKind::Choice { .. } => {
                ctx.emit_info(
                    "settings.apply.reapply",
                    &format!("Reapplying {}", definition.title),
                );
                apply_definition(&mut ctx, definition, None)?;
                applied += 1;
            }
            _ => {}
        }
    }

    if applied == 0 {
        ctx.emit_info(
            "settings.apply.none",
            "No non-persistent settings are currently enabled.",
        );
    } else {
        ctx.emit_success(
            "settings.apply.completed",
            &format!("Reapplied {applied} setting{}", if applied == 1 { "" } else { "s" }),
        );
    }

    Ok(())
}

fn run_internal_apply(
    debug: bool,
    privileged_flag: bool,
    setting_id: &str,
    bool_value: Option<bool>,
    string_value: Option<String>,
    settings_file: Option<PathBuf>,
) -> Result<()> {
    let store = if let Some(path) = settings_file {
        SettingsStore::load_from_path(path)
    } else {
        SettingsStore::load()
    }
    .context("loading settings file for privileged apply")?;

    let mut ctx = SettingsContext::new(store, debug, privileged_flag);

    let definition = SETTINGS
        .iter()
        .find(|definition| definition.id == setting_id)
        .with_context(|| format!("unknown setting id {setting_id}"))?;

    let override_value = match (&definition.kind, bool_value, string_value.as_deref()) {
        (SettingKind::Toggle { .. }, Some(value), _) => Some(ApplyOverride::Bool(value)),
        (SettingKind::Choice { options, .. }, _, Some(value)) => options
            .iter()
            .find(|option| option.value == value)
            .map(ApplyOverride::Choice),
        _ => None,
    };

    apply_definition(&mut ctx, definition, override_value)?;

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

    let mut entries: Vec<CategoryPageItem> = Vec::with_capacity(setting_defs.len() + 1);
    for definition in setting_defs {
        let state = compute_setting_state(ctx, definition);
        entries.push(CategoryPageItem::Setting(SettingItem { definition, state }));
    }

    entries.push(CategoryPageItem::Back);

    match FzfWrapper::select_one(entries)? {
        Some(CategoryPageItem::Setting(item)) => {
            handle_setting(ctx, item.definition, item.state)?;
            ctx.persist()?;
            Ok(true)
        }
        Some(CategoryPageItem::Back) | None => Ok(true),
    }
}

fn handle_search_all(ctx: &mut SettingsContext) -> Result<bool> {
    let mut items = Vec::new();

    for category in CATEGORIES {
        let definitions = registry::settings_for_category(category.id);
        for definition in definitions {
            let state = compute_setting_state(ctx, definition);
            items.push(SearchItem {
                category,
                definition,
                state,
            });
        }
    }

    if items.is_empty() {
        ctx.emit_info("settings.search.empty", "No settings found to search.");
        return Ok(true);
    }

    match FzfWrapper::select_one(items)? {
        Some(selection) => {
            handle_setting(ctx, selection.definition, selection.state)?;
            ctx.persist()?;
        }
        None => {}
    }

    Ok(true)
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
            let choices = vec![
                ToggleChoiceItem {
                    title: definition.title,
                    summary,
                    target_enabled: true,
                    current_enabled: current,
                },
                ToggleChoiceItem {
                    title: definition.title,
                    summary,
                    target_enabled: false,
                    current_enabled: current,
                },
            ];

            match FzfWrapper::select_one(choices)? {
                Some(choice) => {
                    if choice.target_enabled == current {
                        ctx.emit_info(
                            "settings.toggle.noop",
                            &format!(
                                "{} is already {}.",
                                definition.title,
                                if current { "enabled" } else { "disabled" }
                            ),
                        );
                        return Ok(());
                    }

                    ctx.set_bool(*key, choice.target_enabled);
                    if apply.is_some() {
                        apply_definition(ctx, definition, Some(ApplyOverride::Bool(choice.target_enabled)))?;
                    }
                    ctx.emit_success(
                        "settings.toggle.updated",
                        &format!(
                            "{} {}",
                            definition.title,
                            if choice.target_enabled {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        ),
                    );
                }
                None => ctx.emit_info("settings.toggle.cancelled", "No changes made."),
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
                    if apply.is_some() {
                        apply_definition(ctx, definition, Some(ApplyOverride::Choice(choice.option)))?;
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
            ctx.with_definition(definition, |ctx| run(ctx))?;
        }
        SettingKind::Command {
            summary,
            command,
            required,
        } => {
            let mut missing = Vec::new();
            for pkg in *required {
                let installed = pkg.ensure()?;
                if !installed {
                    missing.push(pkg);
                }
            }

            if !missing.is_empty() {
                for pkg in missing {
                    ctx.emit_info(
                        "settings.command.missing",
                        &format!(
                            "{} missing dependency `{}`. {}",
                            definition.title,
                            pkg.name,
                            pkg.install_hint()
                        ),
                    );
                }
                return Ok(());
            }

            ctx.emit_info(
                "settings.command.launching",
                &format!("{}", summary),
            );

            ctx.with_definition(definition, |ctx| {
                match command.style {
                    CommandStyle::Terminal => {
                        cmd(command.program, command.args)
                            .run()
                            .with_context(|| format!("running {}", command.program))?;
                    }
                    CommandStyle::Detached => {
                        Command::new(command.program)
                            .args(command.args)
                            .spawn()
                            .with_context(|| format!("spawning {}", command.program))?;
                    }
                }
                Ok(())
            })?;

            ctx.emit_success(
                "settings.command.completed",
                &format!("Launched {}", definition.title),
            );
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct CategoryItem {
    category: &'static SettingCategory,
    total: usize,
    toggles: usize,
    choices: usize,
    actions: usize,
    commands: usize,
    highlights: [Option<&'static SettingDefinition>; 3],
}

#[derive(Clone, Copy)]
enum CategoryMenuItem {
    SearchAll,
    Category(CategoryItem),
}

#[derive(Clone, Copy)]
struct SettingItem {
    definition: &'static SettingDefinition,
    state: SettingState,
}

#[derive(Clone, Copy)]
enum CategoryPageItem {
    Setting(SettingItem),
    Back,
}

#[derive(Clone, Copy)]
enum SettingState {
    Toggle { enabled: bool },
    Choice { current_index: Option<usize> },
    Action,
    Command,
}

#[derive(Clone, Copy)]
struct ChoiceItem {
    option: &'static SettingOption,
    is_current: bool,
    summary: &'static str,
}

#[derive(Clone, Copy)]
struct ToggleChoiceItem {
    title: &'static str,
    summary: &'static str,
    target_enabled: bool,
    current_enabled: bool,
}

#[derive(Clone, Copy)]
struct SearchItem {
    category: &'static SettingCategory,
    definition: &'static SettingDefinition,
    state: SettingState,
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
        let mut lines = Vec::new();

        lines.push(format!(
            "{} {}",
            char::from(Fa::InfoCircle),
            self.category.description
        ));

        lines.push(String::new());
        lines.push(format!(
            "{} {} total settings",
            char::from(Fa::List),
            self.total
        ));
        lines.push(format!(
            "{} {} toggle{}",
            char::from(Fa::ToggleOn),
            self.toggles,
            if self.toggles == 1 { "" } else { "s" }
        ));
        lines.push(format!(
            "{} {} choice{}",
            char::from(Fa::List),
            self.choices,
            if self.choices == 1 { "" } else { "s" }
        ));
        lines.push(format!(
            "{} {} action{}",
            char::from(Fa::Check),
            self.actions,
            if self.actions == 1 { "" } else { "s" }
        ));
        lines.push(format!(
            "{} {} command{}",
            char::from(Fa::Terminal),
            self.commands,
            if self.commands == 1 { "" } else { "s" }
        ));

        let highlights: Vec<_> = self.highlights.iter().flatten().take(3).collect();

        if !highlights.is_empty() {
            lines.push(String::new());
            lines.push(format!("{} Featured settings:", char::from(Fa::LightbulbO)));

            for definition in highlights {
                lines.push(format!(
                    "  {} {} — {}",
                    char::from(definition.icon),
                    definition.title,
                    setting_summary(definition)
                ));
            }
        }

        FzfPreview::Text(lines.join("\n"))
    }
}

impl FzfSelectable for CategoryMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryMenuItem::SearchAll => {
                format!("{} Search all settings", char::from(Fa::Search))
            }
            CategoryMenuItem::Category(item) => item.fzf_display_text(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            CategoryMenuItem::SearchAll => {
                FzfPreview::Text("Browse and edit any available setting".to_string())
            }
            CategoryMenuItem::Category(item) => item.fzf_preview(),
        }
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
            SettingState::Command => {
                let glyph = match &self.definition.kind {
                    SettingKind::Command { command, .. } => match command.style {
                        CommandStyle::Terminal => Fa::Terminal,
                        CommandStyle::Detached => Fa::ExternalLink,
                    },
                    _ => self.definition.icon,
                };
                format!("{} {}", char::from(glyph), self.definition.title)
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match &self.definition.kind {
            SettingKind::Toggle { summary, .. }
            | SettingKind::Choice { summary, .. }
            | SettingKind::Action { summary, .. }
            | SettingKind::Command { summary, .. } => FzfPreview::Text(summary.to_string()),
        }
    }
}

impl FzfSelectable for CategoryPageItem {
    fn fzf_display_text(&self) -> String {
        match self {
            CategoryPageItem::Setting(item) => item.fzf_display_text(),
            CategoryPageItem::Back => {
                format!("{} Back", char::from(Fa::ArrowLeft))
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            CategoryPageItem::Setting(item) => item.fzf_preview(),
            CategoryPageItem::Back => FzfPreview::Text("Return to categories".to_string()),
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

impl FzfSelectable for ToggleChoiceItem {
    fn fzf_display_text(&self) -> String {
        let glyph = if self.target_enabled {
            Fa::ToggleOn
        } else {
            Fa::ToggleOff
        };
        let action = if self.target_enabled {
            "Enable"
        } else {
            "Disable"
        };
        let current_marker = if self.target_enabled == self.current_enabled {
            " (current)"
        } else {
            ""
        };
        format!(
            "{} {} {}{}",
            char::from(glyph),
            action,
            self.title,
            current_marker
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(self.summary.to_string())
    }
}

impl FzfSelectable for SearchItem {
    fn fzf_display_text(&self) -> String {
        let path = format_setting_path(self.category, self.definition);
        match self.state {
            SettingState::Toggle { enabled } => {
                let glyph = if enabled { Fa::ToggleOn } else { Fa::ToggleOff };
                format!("{} {}", char::from(glyph), path)
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
                format!("{} {}  [{}]", char::from(glyph), path, current_label)
            }
            SettingState::Action => {
                format!("{} {}", char::from(self.definition.icon), path)
            }
            SettingState::Command => {
                let glyph = match &self.definition.kind {
                    SettingKind::Command { command, .. } => match command.style {
                        CommandStyle::Terminal => Fa::Terminal,
                        CommandStyle::Detached => Fa::ExternalLink,
                    },
                    _ => self.definition.icon,
                };
                format!("{} {}", char::from(glyph), path)
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match &self.definition.kind {
            SettingKind::Toggle { summary, .. }
            | SettingKind::Choice { summary, .. }
            | SettingKind::Action { summary, .. }
            | SettingKind::Command { summary, .. } => FzfPreview::Text(summary.to_string()),
        }
    }
}

fn setting_summary(definition: &SettingDefinition) -> &'static str {
    match &definition.kind {
        SettingKind::Toggle { summary, .. } => summary,
        SettingKind::Choice { summary, .. } => summary,
        SettingKind::Action { summary, .. } => summary,
        SettingKind::Command { summary, .. } => summary,
    }
}

fn compute_setting_state(
    ctx: &SettingsContext,
    definition: &'static SettingDefinition,
) -> SettingState {
    match &definition.kind {
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
        SettingKind::Command { .. } => SettingState::Command,
    }
}

fn format_setting_path(category: &SettingCategory, definition: &SettingDefinition) -> String {
    let mut segments = Vec::with_capacity(1 + definition.breadcrumbs.len());
    segments.push(category.title);
    segments.extend(definition.breadcrumbs.iter().copied());
    segments.join(" -> ")
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
