//! Flatpak installer settings
//!
//! Interactive Flatpak app browser and installer.

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// Install Flatpak Apps
// ============================================================================

pub struct InstallFlatpakApps;

impl Setting for InstallFlatpakApps {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.install_flatpak_apps")
            .title("Install Flatpak apps")
            .icon(NerdFont::Download)
            .summary("Browse and install Flatpak applications using an interactive fuzzy finder.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        run_flatpak_installer_action(_ctx)
    }
}

/// Run the interactive Flatpak installer as a settings action
fn run_flatpak_installer_action(_ctx: &mut SettingsContext) -> Result<()> {
    println!("Starting Flatpak app installer...");

    // Check if flatpak is available
    if !is_flatpak_available() {
        anyhow::bail!("Flatpak is not available on this system");
    }

    // List available apps from all remotes with origin column for preview
    let list_command =
        "flatpak remote-ls --app --columns=name,application,description,version,origin";

    // Build human-readable preview command using flatpak remote-info with Nerd Font icons
    let package_icon = NerdFont::Package.to_string();
    let error_icon = NerdFont::Cross.to_string();
    let preview_cmd = format!(
        "sh -c 'remote=\"$(echo \"{{5}}\" | cut -f1)\"; app=\"$(echo \"{{2}}\" | cut -f1)\"; printf \"\\033[1;34m{} %s\\033[0m\\n\" \"$app\"; flatpak remote-info \"$remote\" \"$app\" 2>/dev/null || printf \"\\033[1;31m{} No additional information available\\033[0m\\n\"'",
        package_icon, error_icon
    );

    let result = crate::menu_utils::FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select Flatpak apps to install")
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

            // Confirm installation
            let total = app_ids.len();
            let confirm_msg = format!(
                "Install {} Flatpak app{}?",
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
            install_flatpak_apps(&app_ids)?;

            println!("✓ Flatpak app installation completed successfully!");
        }
        crate::menu_utils::FzfResult::Selected(line) => {
            // Extract the app ID (second column)
            let app_id = line.split('\t').nth(1).unwrap_or(&line).to_string();

            println!("Installing Flatpak app: {}", app_id);
            install_flatpak_apps(&[app_id])?;
            println!("✓ Flatpak app installation completed successfully!");
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

/// Check if flatpak is available on the system
fn is_flatpak_available() -> bool {
    std::process::Command::new("which")
        .arg("flatpak")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Install Flatpak apps
fn install_flatpak_apps(app_ids: &[String]) -> Result<()> {
    if app_ids.is_empty() {
        return Ok(());
    }

    println!("Installing Flatpak apps...");

    for app_id in app_ids {
        println!("Installing {}...", app_id);

        let status = std::process::Command::new("flatpak")
            .arg("install")
            .arg("--assumeyes") // Automatically answer yes to prompts
            .arg("--system") // Install system-wide (default)
            .arg(app_id)
            .status()?;

        if !status.success() {
            eprintln!("Warning: Failed to install {}", app_id);
        }
    }

    Ok(())
}
