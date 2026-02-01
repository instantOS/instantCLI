//! Snap installer settings
//!
//! Interactive Snap app browser and installer.

use anyhow::Result;

use crate::common::package::{PackageManager, install_package_names};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// Install Snap Apps
// ============================================================================

pub struct InstallSnapApps;

impl Setting for InstallSnapApps {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.install_snap_apps")
            .title("Install Snap apps")
            .icon(NerdFont::Download)
            .summary("Browse and install Snap applications using an interactive fuzzy finder.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        run_snap_installer()
    }
}

/// Run the interactive Snap installer
fn run_snap_installer() -> Result<()> {
    println!("Starting Snap app installer...");

    // Check if snap is available
    if !PackageManager::Snap.is_available() {
        anyhow::bail!("Snap is not available on this system");
    }

    // List available apps. Snap doesn't have a "list all" command, so we search for common things or just search "."?
    // The previous implementation used `snap find .` which seems to list some packages.
    // Note: `snap find` without arguments lists featured snaps. `snap find .` lists some general ones.
    // We will use `snap find .` as established.
    // Output format of `snap find .`:
    // Name  Version  Publisher  Notes  Summary
    // ...
    // We want to skip header.
    // We can use awk/sed to format it for FZF if needed, but FZF can handle it.
    // We need to parse it carefully.

    // Command to list packages. We'll use `tail -n +2` to skip the header.
    // We want to display Name, Version, Summary.
    // `snap find .` output is column aligned.

    let list_command = "snap find . 2>/dev/null | tail -n +2";

    // Build human-readable preview command using snap info
    let package_icon = NerdFont::Package.to_string();
    let error_icon = NerdFont::Cross.to_string();
    // {1} is the first column which is the Name in `snap find` output.
    let preview_cmd = format!(
        "sh -c 'app=\"{{1}}\"; printf \"\\033[1;34m{} %s\\033[0m\\n\" \"$app\"; snap info \"$app\" 2>/dev/null || printf \"\\033[1;31m{} No additional information available\\033[0m\\n\"'",
        package_icon, error_icon
    );

    let result = crate::menu_utils::FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select Snap apps to install")
        .args([
            "--preview",
            &preview_cmd,
            "--preview-window",
            "down:65%:wrap",
            "--bind",
            "ctrl-l:clear-screen",
            "--ansi",
            "--no-mouse",
            // `snap find` output is space separated but aligned.
            // We can rely on default awk behavior of fzf for {1} (first field).
            // But we might want to display the whole line.
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

            // Confirm installation
            let total = app_ids.len();
            let confirm_msg = format!(
                "Install {} Snap app{}?",
                total,
                if total == 1 { "" } else { "s" }
            );

            let confirm = crate::menu_utils::FzfWrapper::builder()
                .confirm(&confirm_msg)
                .show_confirmation()?;

            if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
                println!("Installation cancelled.");
                return Ok(());
            }

            // Install apps
            let refs: Vec<&str> = app_ids.iter().map(|s| s.as_str()).collect();
            install_package_names(PackageManager::Snap, &refs)?;

            println!("✓ Snap app installation completed successfully!");
        }
        crate::menu_utils::FzfResult::Selected(line) => {
            // Extract the app name (first column)
            let app_id = line.split_whitespace().next().unwrap_or(&line).to_string();

            println!("Installing Snap app: {}", app_id);
            install_package_names(PackageManager::Snap, &[&app_id])?;
            println!("✓ Snap app installation completed successfully!");
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
