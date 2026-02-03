//! Manage installed Snap apps setting
//!
//! Interactive Snap browser and uninstaller for installed Snap applications.

use anyhow::Result;

use crate::common::package::{PackageManager, uninstall_packages};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

/// Manage installed Snap apps setting.
///
/// This setting allows users to view and uninstall installed Snap applications.
pub struct ManageInstalledSnaps;

impl Setting for ManageInstalledSnaps {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.manage_installed_snaps")
            .title("Manage installed Snaps")
            .icon(NerdFont::Package)
            .summary("View and uninstall installed Snap applications.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        run_installed_snaps_manager()
    }
}

/// Run the interactive installed Snap manager
fn run_installed_snaps_manager() -> Result<()> {
    println!("Starting installed Snap manager...");

    if !PackageManager::Snap.is_available() {
        anyhow::bail!("Snap is not available on this system");
    }

    // List installed apps
    // `snap list` output:
    // Name  Version  Rev  Tracking  Publisher  Notes
    // ...
    // We skip the header.
    let list_command = "snap list 2>/dev/null | tail -n +2";

    let package_icon = NerdFont::Package.to_string();
    let error_icon = NerdFont::Cross.to_string();
    let preview_cmd = format!(
        "sh -c 'app=\"{{1}}\"; printf \"\\033[1;34m{} %s\\033[0m\\n\" \"$app\"; snap info \"$app\" 2>/dev/null || printf \"\\033[1;31m{} No additional information available\\033[0m\\n\"'",
        package_icon, error_icon
    );

    let result = crate::menu_utils::FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select Snap apps to uninstall")
        .args([
            "--preview",
            &preview_cmd,
            "--preview-window",
            "down:65%:wrap",
            "--bind",
            "ctrl-l:clear-screen",
            "--ansi",
            "--no-mouse",
            "--layout",
            "reverse-list",
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
                    // Extract the app name (first column)
                    line.split_whitespace().next().unwrap_or(line).to_string()
                })
                .collect();

            let total = app_ids.len();
            let confirm_msg = format!(
                "Uninstall {} Snap app{}",
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
            uninstall_packages(PackageManager::Snap, &app_refs)?;

            println!("✓ Snap app uninstallation completed successfully!");
        }
        crate::menu_utils::FzfResult::Selected(line) => {
            let app_id = line.split_whitespace().next().unwrap_or(&line).to_string();

            let confirm_msg = format!("Uninstall {}?", app_id);
            let confirm = crate::menu_utils::FzfWrapper::builder()
                .confirm(&confirm_msg)
                .show_confirmation()?;

            if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
                println!("Uninstallation cancelled.");
                return Ok(());
            }

            println!("Uninstalling Snap app: {}", app_id);
            uninstall_packages(PackageManager::Snap, &[&app_id])?;
            println!("✓ Snap app uninstallation completed successfully!");
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
