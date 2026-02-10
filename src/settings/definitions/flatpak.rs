//! Flatpak installer settings
//!
//! Interactive Flatpak app browser and installer.
//! Uses local appstream metadata for fast loading (~15x faster than remote-ls),
//! with fallback to flatpak remote-ls if appstream is unavailable.

use anyhow::{Result, bail};

use crate::common::package::{PackageManager, install_package_names};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper, Header};
use crate::preview::{PreviewId, preview_command_streaming};
use crate::settings::context::SettingsContext;
use crate::settings::deps::FLATPAK;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::fzf_mocha_args;
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

/// Build a shell command that generates the flatpak app list.
/// Uses the internal ins command for fast appstream parsing.
fn flatpak_list_command() -> String {
    "ins settings internal-generate-flatpak-list".to_string()
}

// ============================================================================
// Selection
// ============================================================================

fn select_flatpak_apps() -> Result<Vec<String>> {
    let preview_cmd = preview_command_streaming(PreviewId::Flatpak);
    let list_cmd = flatpak_list_command();

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages")
        .header(Header::fancy("Install Flatpak Apps"))
        .args(fzf_mocha_args())
        .args([
            "--delimiter",
            "\t",
            "--with-nth",
            "2",
            "--preview",
            &preview_cmd,
            "--ansi",
        ])
        .responsive_layout()
        .select_streaming(&list_cmd)?;

    extract_app_ids(result)
}

fn extract_app_ids(result: FzfResult<String>) -> Result<Vec<String>> {
    let lines = match result {
        FzfResult::MultiSelected(lines) => lines,
        FzfResult::Selected(line) => vec![line],
        FzfResult::Cancelled => return Ok(vec![]),
        FzfResult::Error(err) => bail!("App selection failed: {}", err),
    };

    // Extract app_id from "app_id\tname\tdescription" format
    Ok(lines
        .into_iter()
        .map(|line| line.split('\t').next().unwrap_or(&line).to_string())
        .collect())
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

    let msg = format!(
        "Install {} Flatpak app{}?",
        app_ids.len(),
        if app_ids.len() == 1 { "" } else { "s" }
    );

    let result = FzfWrapper::builder().confirm(&msg).show_confirmation()?;
    if !matches!(result, ConfirmResult::Yes) {
        println!("Installation cancelled.");
        return Ok(());
    }

    let refs: Vec<&str> = app_ids.iter().map(|s| s.as_str()).collect();
    install_package_names(PackageManager::Flatpak, &refs)?;

    println!("✓ Flatpak installation completed successfully!");
    Ok(())
}
