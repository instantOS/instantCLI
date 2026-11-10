//! Manage installed Flatpak apps setting
//!
//! Interactive Flatpak browser, runner, and uninstaller for installed Flatpak applications.

use std::collections::BTreeSet;

use anyhow::{Context, Result};

use crate::common::package::{PackageManager, uninstall_packages};
use crate::common::shell::resolve_current_binary;
use crate::menu_utils::{FzfSelectable, FzfWrapper, Header, StreamingCommand};
use crate::settings::context::SettingsContext;
use crate::settings::deps::FLATPAK;
use crate::settings::flatpak_list::FlatpakSelectionPayload;
use crate::settings::installed_packages_manager::{
    retry_package_operation, uninstall_confirmation_message,
};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

/// Action that can be performed on a selected Flatpak
#[derive(Clone)]
pub enum FlatpakAction {
    Run,
    Uninstall,
}

impl FzfSelectable for FlatpakAction {
    fn fzf_display_text(&self) -> String {
        match self {
            FlatpakAction::Run => format!("{} Run", NerdFont::Play),
            FlatpakAction::Uninstall => format!("{} Uninstall", NerdFont::Trash),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            FlatpakAction::Run => "run".to_string(),
            FlatpakAction::Uninstall => "uninstall".to_string(),
        }
    }
}

/// Check if a Flatpak app is already installed
pub fn is_flatpak_installed(app_id: &str) -> bool {
    std::process::Command::new("flatpak")
        .args(["info", app_id])
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Show the action menu for a Flatpak app and execute the selected action
pub fn show_flatpak_action_menu(app_id: &str) -> Result<()> {
    let actions = vec![FlatpakAction::Run, FlatpakAction::Uninstall];

    let action = match FzfWrapper::menu().items(actions).padded().select_one()? {
        crate::menu_utils::DialogOutcome::Submitted(a) => a,
        crate::menu_utils::DialogOutcome::Cancelled => {
            println!("Action selection cancelled.");
            return Ok(());
        }
    };

    match action {
        FlatpakAction::Run => run_flatpak(app_id),
        FlatpakAction::Uninstall => uninstall_flatpaks(&[app_id.to_string()]),
    }
}

/// Manage installed Flatpak apps setting.
///
/// This setting allows users to view and uninstall installed Flatpak applications.
pub struct ManageInstalledFlatpaks;

impl Setting for ManageInstalledFlatpaks {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.manage_installed_flatpaks")
            .title("Manage installed Flatpaks")
            .icon(NerdFont::Package)
            .summary("Run an app or select multiple Flatpak applications to uninstall.")
            .requirements(vec![&FLATPAK])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        run_installed_flatpaks_manager()
    }
}

/// Run the interactive installed Flatpak manager
fn run_installed_flatpaks_manager() -> Result<()> {
    println!("Starting installed Flatpak manager...");

    loop {
        let list_command = StreamingCommand::new(resolve_current_binary())
            .arg("settings")
            .arg("internal-generate-installed-flatpak-list");
        let result = FzfWrapper::builder()
            .prompt("Select Flatpak apps")
            .header(Header::fancy("Manage Installed Flatpaks"))
            .responsive_layout()
            .command::<FlatpakSelectionPayload, _>(list_command)
            .select_many()?;

        let app_ids: Vec<String> = match result {
            crate::menu_utils::DialogOutcome::Submitted(selection) => selection
                .items
                .into_iter()
                .map(|row| row.payload.app_id)
                .collect(),
            crate::menu_utils::DialogOutcome::Cancelled => {
                println!("App selection cancelled.");
                return Ok(());
            }
        };

        match app_ids.as_slice() {
            [] => return Ok(()),
            [app_id] => show_flatpak_action_menu(app_id)?,
            _ => uninstall_flatpaks(&app_ids)?,
        }
    }
}

/// Run a Flatpak application
pub fn run_flatpak(app_id: &str) -> Result<()> {
    println!("Starting Flatpak app: {}...", app_id);

    let status = std::process::Command::new("flatpak")
        .args(["run", app_id])
        .status()
        .with_context(|| format!("Failed to run Flatpak app: {}", app_id))?;

    if status.success() {
        println!("{} Flatpak app exited successfully.", NerdFont::Check);
    } else {
        println!("Flatpak app exited with status: {:?}", status.code());
    }

    Ok(())
}

/// Confirm and uninstall selected apps in one Flatpak transaction.
fn uninstall_flatpaks(app_ids: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    let app_ids: Vec<String> = app_ids
        .iter()
        .filter(|app_id| seen.insert(app_id.as_str()))
        .cloned()
        .collect();
    if app_ids.is_empty() {
        return Ok(());
    }

    let confirm_msg = uninstall_confirmation_message(PackageManager::Flatpak, &app_ids);
    let confirm = FzfWrapper::builder()
        .confirm(&confirm_msg)
        .yes_text("Uninstall")
        .no_text("Cancel")
        .confirm_dialog()?;

    if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
        println!("Uninstallation cancelled.");
        return Ok(());
    }

    let refs: Vec<&str> = app_ids.iter().map(String::as_str).collect();
    if !retry_package_operation(PackageManager::Flatpak, || {
        uninstall_packages(PackageManager::Flatpak, &refs)
    })? {
        return Ok(());
    }
    println!(
        "{} Flatpak uninstallation completed successfully!",
        NerdFont::Check
    );
    Ok(())
}
