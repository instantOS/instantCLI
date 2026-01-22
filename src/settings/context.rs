use std::process::Command;

use anyhow::{Result, bail};
use duct::cmd;
use sudo::RunningAs;

use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;

use super::sources;
use super::store::{
    BoolSettingKey, IntSettingKey, OptionalStringSettingKey, SettingsStore, StringSettingKey,
};

#[derive(Debug)]
pub struct SettingsContext {
    store: SettingsStore,
    dirty: bool,
    debug: bool,
    privileged: bool,
    no_notifications: bool,
}

impl SettingsContext {
    pub fn new(store: SettingsStore, debug: bool, privileged_flag: bool) -> Self {
        let mut ctx = Self {
            store,
            dirty: false,
            debug,
            privileged: privileged_flag || matches!(sudo::check(), RunningAs::Root),
            no_notifications: false,
        };

        ctx.sync_external_states();
        ctx
    }

    pub fn new_with_notifications_disabled(
        store: SettingsStore,
        debug: bool,
        privileged_flag: bool,
    ) -> Self {
        let mut ctx = Self {
            store,
            dirty: false,
            debug,
            privileged: privileged_flag || matches!(sudo::check(), RunningAs::Root),
            no_notifications: true,
        };

        ctx.sync_external_states();
        ctx
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn is_privileged(&self) -> bool {
        self.privileged
    }

    pub fn bool(&self, key: BoolSettingKey) -> bool {
        if let Some(source) = sources::source_for(&key) {
            match source.current() {
                Ok(value) => value,
                Err(err) => {
                    emit(
                        Level::Warn,
                        "settings.state.read_failed",
                        &format!(
                            "{} Failed to read state for '{}': {err}",
                            char::from(NerdFont::Warning),
                            key.key
                        ),
                        None,
                    );
                    self.store.bool(key)
                }
            }
        } else {
            self.store.bool(key)
        }
    }

    pub fn set_bool(&mut self, key: BoolSettingKey, value: bool) {
        if self.store.bool(key) != value {
            self.store.set_bool(key, value);
            self.dirty = true;
        }
    }

    pub fn string(&self, key: StringSettingKey) -> String {
        if let Some(source) = sources::string_source_for(&key) {
            match source.current() {
                Ok(value) => value,
                Err(err) => {
                    emit(
                        Level::Warn,
                        "settings.state.read_failed",
                        &format!(
                            "{} Failed to read state for '{}': {err}",
                            char::from(NerdFont::Warning),
                            key.key
                        ),
                        None,
                    );
                    self.store.string(key)
                }
            }
        } else {
            self.store.string(key)
        }
    }

    pub fn set_string(&mut self, key: StringSettingKey, value: &str) {
        if self.store.string(key) != value {
            self.store.set_string(key, value);
            self.dirty = true;
        }
    }

    pub fn int(&self, key: IntSettingKey) -> i64 {
        self.store.int(key)
    }

    pub fn set_int(&mut self, key: IntSettingKey, value: i64) {
        if self.store.int(key) != value {
            self.store.set_int(key, value);
            self.dirty = true;
        }
    }

    pub fn optional_string(&self, key: OptionalStringSettingKey) -> Option<String> {
        self.store.optional_string(key)
    }

    pub fn set_optional_string<S: Into<String>>(
        &mut self,
        key: OptionalStringSettingKey,
        value: Option<S>,
    ) {
        self.store.set_optional_string(key, value);
        self.dirty = true;
    }

    pub fn contains(&self, key: &str) -> bool {
        self.store.contains(key)
    }

    fn sync_external_states(&mut self) {
        for &(key_ref, source) in sources::all_bool_sources() {
            let key = *key_ref;
            if let Err(err) = self.update_bool_from_source(key, source) {
                emit(
                    Level::Warn,
                    "settings.state.sync_failed",
                    &format!(
                        "{} Failed to synchronize state for '{}': {err}",
                        char::from(NerdFont::Warning),
                        key.key
                    ),
                    None,
                );
            }
        }

        for &(key_ref, source) in sources::all_string_sources() {
            let key = *key_ref;
            if let Err(err) = self.update_string_from_source(key, source) {
                emit(
                    Level::Warn,
                    "settings.state.sync_failed",
                    &format!(
                        "{} Failed to synchronize state for '{}': {err}",
                        char::from(NerdFont::Warning),
                        key.key
                    ),
                    None,
                );
            }
        }
    }

    fn update_bool_from_source(
        &mut self,
        key: BoolSettingKey,
        source: &'static dyn sources::BoolStateSource,
    ) -> Result<bool> {
        let current = source.current()?;
        if self.store.bool(key) != current {
            self.store.set_bool(key, current);
            self.dirty = true;
        }
        Ok(current)
    }

    fn update_string_from_source(
        &mut self,
        key: StringSettingKey,
        source: &'static dyn sources::StringStateSource,
    ) -> Result<String> {
        let current = source.current()?;
        if self.store.string(key) != current {
            self.store.set_string(key, &current);
            self.dirty = true;
        }
        Ok(current)
    }

    pub fn refresh_bool_source(&mut self, key: BoolSettingKey) -> Result<bool> {
        if let Some(source) = sources::source_for(&key) {
            match self.update_bool_from_source(key, source) {
                Ok(value) => Ok(value),
                Err(err) => {
                    emit(
                        Level::Warn,
                        "settings.state.refresh_failed",
                        &format!(
                            "{} Failed to refresh state for '{}': {err}",
                            char::from(NerdFont::Warning),
                            key.key
                        ),
                        None,
                    );
                    Ok(self.store.bool(key))
                }
            }
        } else {
            Ok(self.store.bool(key))
        }
    }

    pub fn refresh_string_source(&mut self, key: StringSettingKey) -> Result<String> {
        if let Some(source) = sources::string_source_for(&key) {
            match self.update_string_from_source(key, source) {
                Ok(value) => Ok(value),
                Err(err) => {
                    emit(
                        Level::Warn,
                        "settings.state.refresh_failed",
                        &format!(
                            "{} Failed to refresh state for '{}': {err}",
                            char::from(NerdFont::Warning),
                            key.key
                        ),
                        None,
                    );
                    Ok(self.store.string(key))
                }
            }
        } else {
            Ok(self.store.string(key))
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
        if self.no_notifications {
            // Skip notifications entirely when no_notifications is true
            return;
        }

        if self.debug {
            let message = format!("{} {summary}: {body}", char::from(NerdFont::Info));
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

    pub fn show_message(&self, message: &str) {
        // Best-effort; user feedback in TUI context
        let _ = FzfWrapper::message(message);
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

    pub fn emit_unsupported(&self, code: &str, message: &str) {
        self.emit_info(code, message);
        self.show_message(message);
    }

    pub fn emit_failure(&self, code: &str, message: &str) {
        emit(Level::Warn, code, message, None);
        self.show_message(message);
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

    // fn invoke_privileged(&mut self, value: PrivilegedValue) -> Result<()> {
    //     if self.privileged {
    //         return Ok(());
    //     }
    //
    //     let definition = self.current_definition.ok_or_else(|| {
    //         anyhow::anyhow!("no active setting definition for privilege escalation")
    //     })?;
    //
    //     let exe = env::current_exe().context("locating executable")?;
    //     let settings_path = self.store.path().to_path_buf();
    //
    //     let mut command = Command::new("/usr/bin/sudo");
    //     command.arg(&exe);
    //     if self.debug {
    //         command.arg("--debug");
    //     }
    //     command.arg("--internal-privileged-mode");
    //     command.arg("settings");
    //     command.arg("internal-apply");
    //     command.arg("--setting-id");
    //     command.arg(definition.id);
    //     command.arg("--settings-file");
    //     command.arg(&settings_path);
    //     match value {
    //         PrivilegedValue::Bool(v) => {
    //             command.arg("--bool-value");
    //             command.arg(if v { "true" } else { "false" });
    //         }
    //         PrivilegedValue::Choice(v) => {
    //             command.arg("--string-value");
    //             command.arg(v);
    //         }
    //     }
    //
    //     let status = command
    //         .status()
    //         .with_context(|| format!("escalating setting {}", definition.id))?;
    //
    //     if !status.success() {
    //         bail!(
    //             "privileged apply for {} exited with status {:?}",
    //             definition.id,
    //             status.code()
    //         );
    //     }
    //
    //     Ok(())
    // }
}
