//! Flatpak installer settings
//!
//! Interactive Flatpak app browser and installer.

use anyhow::{Result, bail};

use crate::common::package::{PackageManager, install_package_names};
use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::deps::FLATPAK;
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

    fn apply(&self, _ctx: &mut SettingsContext) -> Result<()> {
        run_flatpak_installer()
    }
}

// ============================================================================
// Commands
// ============================================================================

fn flatpak_list_command() -> &'static str {
    "flatpak remote-ls --app --columns=name,application,description,version,origin"
}

fn flatpak_preview_command() -> String {
    let package_icon = NerdFont::Package.to_string();
    let error_icon = NerdFont::Cross.to_string();

    format!(
        "sh -c 'remote=\"$(echo \"{{5}}\" | cut -f1)\"; app=\"$(echo \"{{2}}\" | cut -f1)\"; \
         printf \"\\033[1;34m{} %s\\033[0m\\n\" \"$app\"; \
         flatpak remote-info --system \"$remote\" \"$app\" 2>/dev/null || \
         flatpak remote-info --user \"$remote\" \"$app\" 2>/dev/null || \
         printf \"\\033[1;31m{} No additional information available\\033[0m\\n\"'",
        package_icon, error_icon
    )
}

// ============================================================================
// Selection
// ============================================================================

fn select_flatpak_apps() -> Result<Vec<String>> {
    let preview = flatpak_preview_command();

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select Flatpak apps to install")
        .args([
            "--preview",
            &preview,
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
        .select_streaming(flatpak_list_command())?;

    normalize_fzf_to_app_ids(result)
}

fn normalize_fzf_to_app_ids(result: FzfResult<String>) -> Result<Vec<String>> {
    let lines = match result {
        FzfResult::MultiSelected(lines) => lines,
        FzfResult::Selected(line) => vec![line],
        FzfResult::Cancelled => return Ok(vec![]),
        FzfResult::Error(err) => bail!("App selection failed: {}", err),
    };

    Ok(lines
        .iter()
        .map(|line| parse_app_id(line))
        .collect())
}

fn parse_app_id(line: &str) -> String {
    line.split('\t').nth(1).unwrap_or(line).to_string()
}

// ============================================================================
// Confirmation
// ============================================================================

fn confirm_install(count: usize) -> Result<bool> {
    let msg = format!(
        "Install {} Flatpak app{}?",
        count,
        if count == 1 { "" } else { "s" }
    );

    let result = FzfWrapper::builder().confirm(&msg).show_confirmation()?;
    Ok(matches!(result, crate::menu_utils::ConfirmResult::Yes))
}

// ============================================================================
// Installation
// ============================================================================

fn install_flatpak_apps(app_ids: &[String]) -> Result<()> {
    let refs: Vec<&str> = app_ids.iter().map(|s| s.as_str()).collect();
    install_package_names(PackageManager::Flatpak, &refs)
}

// ============================================================================
// Orchestration
// ============================================================================

fn run_flatpak_installer() -> Result<()> {
    println!("Starting Flatpak app installer...");

    let app_ids = select_flatpak_apps()?;
    if app_ids.is_empty() {
        println!("No apps selected.");
        return Ok(());
    }

    if !confirm_install(app_ids.len())? {
        println!("Installation cancelled.");
        return Ok(());
    }

    install_flatpak_apps(&app_ids)?;
    println!("✓ Flatpak app installation completed successfully!");
    Ok(())
}
