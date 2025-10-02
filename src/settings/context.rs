use std::{env, process::Command};

use anyhow::{Context, Result, bail};
use duct::cmd;
use sudo::RunningAs;

use crate::common::requirements::RequiredPackage;
use crate::ui::prelude::*;

use super::registry::{SettingDefinition, SettingKind, SettingOption};
use super::store::{BoolSettingKey, SettingsStore, StringSettingKey};

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

    pub fn ensure_packages(&mut self, packages: &[RequiredPackage]) -> Result<bool> {
        let mut all_installed = true;
        for package in packages {
            if !package.ensure()? {
                all_installed = false;
            }
        }
        Ok(all_installed)
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
                char::from(NerdFont::Info),
                format!("{summary}: {body}")
            );
            emit(Level::Debug, "settings.notify", &message, None);
            return;
        }

        let result = cmd!("notify-send", summary, body).run();
        if let Err(err) = result {
            let message = format!(
                "{} Failed to send notification: {err}",
                char::from(NerdFont::Warning)
            );
            emit(Level::Debug, "settings.notify.error", &message, None);
        }
    }

    pub fn emit_success(&self, code: &str, message: &str) {
        emit(
            Level::Success,
            code,
            &format!("{} {message}", char::from(NerdFont::Check)),
            None,
        );
    }

    pub fn emit_info(&self, code: &str, message: &str) {
        emit(
            Level::Info,
            code,
            &format!("{} {message}", char::from(NerdFont::Info)),
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

    pub fn run_command_as_root<I, S>(&self, program: S, args: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: AsRef<std::ffi::OsStr>,
        S: AsRef<std::ffi::OsStr>,
    {
        let program_os = program.as_ref().to_owned();
        let status = if self.privileged {
            let mut command = Command::new(&program_os);
            command.args(args);
            command.status()
        } else {
            let mut command = Command::new("/usr/bin/sudo");
            command.arg(&program_os);
            command.args(args);
            command.status()
        }?;

        if !status.success() {
            bail!(
                "command {:?} failed with status {:?}",
                program_os,
                status.code()
            );
        }

        Ok(())
    }

    fn invoke_privileged(&mut self, value: PrivilegedValue) -> Result<()> {
        if self.privileged {
            return Ok(());
        }

        let definition = self.current_definition.ok_or_else(|| {
            anyhow::anyhow!("no active setting definition for privilege escalation")
        })?;

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
            bail!(
                "privileged apply for {} exited with status {:?}",
                definition.id,
                status.code()
            );
        }

        Ok(())
    }

    pub fn store(&self) -> &SettingsStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut SettingsStore {
        &mut self.store
    }
}

enum PrivilegedValue {
    Bool(bool),
    Choice(String),
}

pub fn format_icon(icon: NerdFont) -> String {
    format!("  {}  ", char::from(icon))
}

pub fn make_apply_override(
    definition: &'static SettingDefinition,
    bool_value: Option<bool>,
    string_value: Option<String>,
) -> Option<ApplyOverride> {
    match (&definition.kind, bool_value, string_value.as_deref()) {
        (crate::settings::registry::SettingKind::Toggle { .. }, Some(value), _) => {
            Some(ApplyOverride::Bool(value))
        }
        (crate::settings::registry::SettingKind::Choice { options, .. }, _, Some(value)) => options
            .iter()
            .find(|option| option.value == value)
            .map(ApplyOverride::Choice),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub enum ApplyOverride {
    Bool(bool),
    Choice(&'static SettingOption),
}

pub fn apply_definition(
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
            if let Some(option) = options
                .iter()
                .find(|candidate| candidate.value == current_value)
            {
                ctx.with_definition(definition, |ctx| apply_fn(ctx, option))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

pub fn select_one_with_style_at<T>(items: Vec<T>, initial_index: Option<usize>) -> Result<Option<T>>
where
    T: crate::fzf_wrapper::FzfSelectable + Clone,
{
    let mut builder = crate::fzf_wrapper::FzfWrapper::builder().args(["--gap-line=-", "--gap"]);
    if let Some(index) = initial_index {
        builder = builder.initial_index(index);
    }

    match builder.select(items)? {
        crate::fzf_wrapper::FzfResult::Selected(item) => Ok(Some(item)),
        _ => Ok(None),
    }
}

pub fn select_one_with_style<T>(items: Vec<T>) -> Result<Option<T>>
where
    T: crate::fzf_wrapper::FzfSelectable + Clone,
{
    select_one_with_style_at(items, None)
}
