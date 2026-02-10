//! Manage installed Flatpak apps setting
//!
//! Interactive Flatpak browser, runner, and uninstaller for installed Flatpak applications.

use anyhow::{Context, Result};

use crate::common::package::{PackageManager, uninstall_packages};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, select_one_with_style};
use crate::settings::context::SettingsContext;
use crate::settings::deps::FLATPAK;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

/// Action that can be performed on a selected Flatpak
#[derive(Clone)]
enum FlatpakAction {
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

    fn fzf_search_keywords(&self) -> &[&str] {
        match self {
            FlatpakAction::Run => &["run"],
            FlatpakAction::Uninstall => &["uninstall"],
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

    // List installed apps with relevant columns
    let list_command = "flatpak list --app --columns=name,application,version,origin,size";

    let package_icon = NerdFont::Package.to_string();
    let error_icon = NerdFont::Cross.to_string();
    let preview_cmd = format!(
        "sh -c 'app=\"$(echo \"{{2}}\" | cut -f1)\"; printf \"\\033[1;34m{} %s\\033[0m\\n\" \"$app\"; flatpak info \"$app\" 2>/dev/null || printf \"\\033[1;31m{} No additional information available\\033[0m\\n\"'",
        package_icon, error_icon
    );

    let result = FzfWrapper::builder()
        .prompt("Select a Flatpak app")
        .args([
            "--preview",
            &preview_cmd,
            "--preview-window",
            "down:65%:wrap",
            "--bind",
            "ctrl-l:clear-screen",
            "--ansi",
            "--no-mouse",
            "--delimiter",
            "\\t",
            "--with-nth",
            "1,3..",
        ])
        .select_streaming(list_command)?;

    let app_id = match result {
        FzfResult::Selected(line) => line.split('\t').nth(1).unwrap_or(&line).to_string(),
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
        None => {
            println!("Action selection cancelled.");
            return Ok(());
        }
    };

    match action {
        FlatpakAction::Run => run_flatpak(&app_id),
        FlatpakAction::Uninstall => uninstall_flatpak(&app_id),
    }
}

/// Run a Flatpak application
fn run_flatpak(app_id: &str) -> Result<()> {
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
fn uninstall_flatpak(app_id: &str) -> Result<()> {
    let confirm_msg = format!("Uninstall {}?", app_id);
    let confirm = FzfWrapper::builder()
        .confirm(&confirm_msg)
        .show_confirmation()?;

    if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
        println!("Uninstallation cancelled.");
        return Ok(());
    }

    println!("Uninstalling Flatpak app: {}", app_id);
    uninstall_packages(PackageManager::Flatpak, &[app_id])?;
    println!("✓ Flatpak app uninstallation completed successfully!");

    Ok(())
}
