//! Manage installed Flatpak apps setting
//!
//! Interactive Flatpak browser, runner, and uninstaller for installed Flatpak applications.

use anyhow::{Context, Result};

use crate::common::package::{PackageManager, uninstall_packages};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, select_one_with_style};
use crate::preview::{PreviewId, preview_command_streaming};
use crate::settings::context::SettingsContext;
use crate::settings::deps::FLATPAK;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::fzf_mocha_args;
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

    let action = match select_one_with_style(actions)? {
        Some(a) => a,
        None => {
            println!("Action selection cancelled.");
            return Ok(());
        }
    };

    match action {
        FlatpakAction::Run => run_flatpak(app_id),
        FlatpakAction::Uninstall => {
            uninstall_flatpak(app_id)?;
            Ok(())
        }
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
            .summary("Run or uninstall installed Flatpak applications.")
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

    // List installed apps with app_id first for preview compatibility
    // Format: application\tname\tversion\torigin\tsize
    let list_command = "flatpak list --app --columns=application,name,version,origin,size";
    let preview_cmd = preview_command_streaming(PreviewId::Flatpak);

    loop {
        let result = FzfWrapper::builder()
            .prompt("Select a Flatpak app")
            .header(Header::fancy("Manage Installed Flatpaks"))
            .args(fzf_mocha_args())
            .args([
                "--delimiter",
                "\t",
                "--with-nth",
                "2,3..", // Show name and remaining fields, hide app_id from display
                "--preview",
                &preview_cmd,
                "--ansi",
            ])
            .responsive_layout()
            .select_streaming(list_command)?;

        let app_id = match result {
            FzfResult::Selected(line) => line.split('\t').next().unwrap_or(&line).to_string(),
            FzfResult::Cancelled => {
                println!("App selection cancelled.");
                return Ok(());
            }
            FzfResult::Error(err) => {
                anyhow::bail!("App selection failed: {}", err);
            }
            _ => {
                println!("No app selected.");
                return Ok(());
            }
        };

        // Show action menu for the selected app
        let actions = vec![FlatpakAction::Run, FlatpakAction::Uninstall];

        let action = match select_one_with_style(actions)? {
            Some(a) => a,
            None => continue, // Action cancelled, return to list
        };

        match action {
            FlatpakAction::Run => run_flatpak(&app_id)?,
            FlatpakAction::Uninstall => {
                if uninstall_flatpak(&app_id)? {
                    // Uninstall completed, list will refresh on next iteration
                }
                // Whether cancelled or completed, return to list
            }
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
        println!("✓ Flatpak app exited successfully.");
    } else {
        println!("Flatpak app exited with status: {:?}", status.code());
    }

    Ok(())
}

/// Uninstall a Flatpak application
pub fn uninstall_flatpak(app_id: &str) -> Result<bool> {
    let confirm_msg = format!("Uninstall {}?", app_id);
    let confirm = FzfWrapper::builder()
        .confirm(&confirm_msg)
        .show_confirmation()?;

    if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
        println!("Uninstallation cancelled.");
        return Ok(false);
    }

    println!("Uninstalling Flatpak app: {}", app_id);
    uninstall_packages(PackageManager::Flatpak, &[app_id])?;
    println!("✓ Flatpak app uninstallation completed successfully!");

    Ok(true)
}
