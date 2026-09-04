//! Flatpak installer settings
//!
//! Interactive Flatpak app browser and installer.
//! Uses local appstream metadata for fast loading (~15x faster than remote-ls),
//! with fallback to flatpak remote-ls if appstream is unavailable.

use anyhow::Result;

use crate::common::package::{PackageManager, install_package_names};
use crate::common::shell::resolve_current_binary;
use crate::menu_utils::{
    ConfirmResult, DecodedStreamingMenuItem, FzfWrapper, Header, MenuSelection, StreamingCommand,
};
use crate::settings::context::SettingsContext;
use crate::settings::definitions::installed_flatpaks::{
    is_flatpak_installed, show_flatpak_action_menu,
};
use crate::settings::deps::FLATPAK;
use crate::settings::flatpak_list::FlatpakSelectionPayload;
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
            .requirements(vec![&FLATPAK])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        run_flatpak_installer(ctx.debug())
    }
}

// ============================================================================
// Commands
// ============================================================================

/// Build a typed command that generates the flatpak app list.
/// Uses the internal ins command for fast appstream parsing.
fn flatpak_list_command() -> StreamingCommand {
    StreamingCommand::new(resolve_current_binary())
        .arg("settings")
        .arg("internal-generate-flatpak-list")
}

// ============================================================================
// Selection
// ============================================================================

fn select_flatpak_apps() -> Result<Vec<String>> {
    let list_cmd = flatpak_list_command();

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages")
        .header(Header::fancy("Install Flatpak Apps"))
        .responsive_layout()
        .select_encoded_streaming(list_cmd)?;

    extract_app_ids(result)
}

fn extract_app_ids(
    result: crate::menu_utils::DialogOutcome<
        MenuSelection<DecodedStreamingMenuItem<FlatpakSelectionPayload>>,
    >,
) -> Result<Vec<String>> {
    let rows = match result {
        crate::menu_utils::DialogOutcome::Submitted(rows) => rows.items,
        crate::menu_utils::DialogOutcome::Cancelled => return Ok(vec![]),
    };

    Ok(rows.into_iter().map(|row| row.payload.app_id).collect())
}

// ============================================================================
// Orchestration
// ============================================================================

fn run_flatpak_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Flatpak app installer...");
    }

    let app_ids = select_flatpak_apps()?;
    if app_ids.is_empty() {
        println!("No apps selected.");
        return Ok(());
    }

    // Check which apps are already installed
    let (installed, not_installed): (Vec<String>, Vec<String>) = app_ids
        .into_iter()
        .partition(|app_id| is_flatpak_installed(app_id));

    // Handle single app selection that's already installed - seamless DX
    if installed.len() == 1 && not_installed.is_empty() {
        if debug {
            println!(
                "App {} is already installed, showing action menu",
                installed[0]
            );
        }
        return show_flatpak_action_menu(&installed[0]);
    }

    // Install not-installed apps first
    if !not_installed.is_empty() {
        let msg = format!(
            "Install {} Flatpak app{}?",
            not_installed.len(),
            if not_installed.len() == 1 { "" } else { "s" }
        );

        let result = FzfWrapper::builder().confirm(&msg).confirm_dialog()?;
        if !matches!(result, ConfirmResult::Yes) {
            println!("Installation cancelled.");
            // Still show action menu for already installed apps if any
            if !installed.is_empty() {
                println!(
                    "\n{} app(s) already installed - showing action menu(s)",
                    installed.len()
                );
                for app_id in installed {
                    show_flatpak_action_menu(&app_id)?;
                }
            }
            return Ok(());
        }

        let refs: Vec<&str> = not_installed.iter().map(|s| s.as_str()).collect();
        install_package_names(PackageManager::Flatpak, &refs)?;

        println!("✓ Flatpak installation completed successfully!");
    }

    // Show action menu for already installed apps
    if !installed.is_empty() {
        if not_installed.is_empty() {
            println!("\nSelected app(s) already installed:");
        } else {
            println!("\nSome selected app(s) were already installed:");
        }

        for app_id in installed {
            show_flatpak_action_menu(&app_id)?;
        }
    }

    Ok(())
}
