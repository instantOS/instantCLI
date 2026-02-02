//! Manage installed Flatpak apps setting
//!
//! Interactive Flatpak browser and uninstaller for installed Flatpak applications.

use anyhow::Result;

use crate::common::package::{PackageManager, uninstall_packages};
use crate::settings::context::SettingsContext;
use crate::settings::deps::FLATPAK;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

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
            .summary("View and uninstall installed Flatpak applications.")
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

    let result = crate::menu_utils::FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select Flatpak apps to uninstall")
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

    match result {
        crate::menu_utils::FzfResult::MultiSelected(lines) => {
            if lines.is_empty() {
                println!("No apps selected.");
                return Ok(());
            }

            let app_ids: Vec<String> = lines
                .iter()
                .map(|line| {
                    // Extract the app ID (second column)
                    line.split('\t').nth(1).unwrap_or(line).to_string()
                })
                .collect();

            let total = app_ids.len();
            let confirm_msg = format!(
                "Uninstall {} Flatpak app{}?",
                total,
                if total == 1 { "" } else { "s" }
            );

            let confirm = crate::menu_utils::FzfWrapper::builder()
                .confirm(&confirm_msg)
                .show_confirmation()?;

            if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
                println!("Uninstallation cancelled.");
                return Ok(());
            }

            let app_refs: Vec<&str> = app_ids.iter().map(|s| s.as_str()).collect();
            uninstall_packages(PackageManager::Flatpak, &app_refs)?;

            println!("✓ Flatpak app uninstallation completed successfully!");
        }
        crate::menu_utils::FzfResult::Selected(line) => {
            let app_id = line.split('\t').nth(1).unwrap_or(&line).to_string();

            let confirm_msg = format!("Uninstall {}?", app_id);
            let confirm = crate::menu_utils::FzfWrapper::builder()
                .confirm(&confirm_msg)
                .show_confirmation()?;

            if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
                println!("Uninstallation cancelled.");
                return Ok(());
            }

            println!("Uninstalling Flatpak app: {}", app_id);
            uninstall_packages(PackageManager::Flatpak, &[&app_id])?;
            println!("✓ Flatpak app uninstallation completed successfully!");
        }
        crate::menu_utils::FzfResult::Cancelled => {
            println!("App selection cancelled.");
        }
        crate::menu_utils::FzfResult::Error(err) => {
            anyhow::bail!("App selection failed: {}", err);
        }
    }

    Ok(())
}
