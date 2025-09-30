use anyhow::Result;
use clap::{Subcommand, ValueHint};

use super::apply;

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
        settings_file: Option<std::path::PathBuf>,
    },
}

pub fn dispatch_settings_command(
    debug: bool,
    privileged_flag: bool,
    command: Option<SettingsCommands>,
) -> Result<()> {
    match command {
        None => super::ui::run_settings_ui(debug, privileged_flag),
        Some(SettingsCommands::Apply) => apply::run_nonpersistent_apply(debug, privileged_flag),
        Some(SettingsCommands::InternalApply {
            setting_id,
            bool_value,
            string_value,
            settings_file,
        }) => apply::run_internal_apply(
            debug,
            privileged_flag,
            &setting_id,
            bool_value,
            string_value,
            settings_file,
        ),
    }
}
