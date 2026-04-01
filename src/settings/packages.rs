//! Package installer for the settings TUI
//!
//! Provides interactive package installation using fzf with streaming for performance.

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, detect_aur_helper, install_package_names};
use crate::menu_utils::{ConfirmResult, DecodedStreamingMenuItem, FzfResult, FzfWrapper, Header};
use crate::settings::package_list::{self, PackageSelectionPayload};
use crate::ui::catppuccin::fzf_mocha_args;
use anyhow::{Context, Result};

use super::SettingsContext;

/// Run the interactive package installer as a settings action.
/// Dispatches to the appropriate package manager based on the detected OS.
pub fn run_package_installer_action(ctx: &mut SettingsContext) -> Result<()> {
    let os = OperatingSystem::detect();
    let debug = ctx.debug();

    if os.in_family(&OperatingSystem::Arch) {
        run_arch_installer(debug)
    } else if let Some(manager) = os.native_package_manager() {
        run_simple_installer(manager, debug)
    } else {
        anyhow::bail!(
            "Package installer not supported on this system ({})",
            os.name()
        )
    }
}

// ============================================================================
// Simple Package Installer (Debian, Fedora, openSUSE, Termux)
// ============================================================================

/// Run a simple single-manager package installer.
fn run_simple_installer(manager: PackageManager, debug: bool) -> Result<()> {
    if debug {
        println!("Starting {} package installer...", manager);
    }

    if !manager.is_available() {
        anyhow::bail!("{} is not available on this system", manager);
    }

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages")
        .header(Header::fancy("Install Packages"))
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_encoded_streaming(package_list::available_command(manager))
        .context("Failed to run package selector")?;

    handle_install_result(
        result,
        |packages| install_package_names(manager, packages),
        debug,
    )
}

/// Run the interactive Snap installer.
pub fn run_snap_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Snap package installer...");
    }

    if !PackageManager::Snap.is_available() {
        anyhow::bail!("Snap is not available on this system");
    }

    // Build reload command that searches snaps as user types
    // Uses --phony to disable local filtering and rely on snap find results
    // Output format: name\tversion\tpublisher\tsummary
    let reload_cmd = package_list::snap_search_reload_command();

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Search snaps")
        .header(Header::fancy("Type to search Snap Store"))
        .args(fzf_mocha_args())
        .args([
            "--bind",
            &format!("change:reload:{}", reload_cmd),
            "--phony",
        ])
        .responsive_layout()
        .select_encoded_streaming(package_list::snap_search_command(None))
        .context("Failed to run snap selector")?;

    handle_install_result(
        result,
        |packages| install_package_names(PackageManager::Snap, packages),
        debug,
    )
}

// ============================================================================
// Arch Package Installer (Pacman + AUR)
// ============================================================================

/// Run the Arch package installer with support for both pacman and AUR.
fn run_arch_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Arch package installer...");
    }

    let aur_helper = detect_aur_helper();
    let has_pacman = PackageManager::Pacman.is_available();

    if !has_pacman && aur_helper.is_none() {
        anyhow::bail!("Neither pacman nor an AUR helper is available on this system");
    }

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages")
        .header(Header::fancy("Install Packages"))
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_encoded_streaming(package_list::arch_available_command())
        .context("Failed to run package selector")?;

    handle_arch_install_result(result, detect_aur_helper(), debug)
}

/// Handle Arch install result, splitting packages by source.
fn handle_arch_install_result(
    result: FzfResult<DecodedStreamingMenuItem<PackageSelectionPayload>>,
    aur_helper: Option<&str>,
    debug: bool,
) -> Result<()> {
    match result {
        FzfResult::MultiSelected(rows) if !rows.is_empty() => {
            let (repo_pkgs, aur_pkgs) = parse_arch_selections(&rows);

            if debug {
                println!("Repo packages: {:?}", repo_pkgs);
                println!("AUR packages: {:?}", aur_pkgs);
            }

            let total = repo_pkgs.len() + aur_pkgs.len();
            let msg = format!(
                "Install {} package{} ({} Repo, {} AUR)?",
                total,
                if total == 1 { "" } else { "s" },
                repo_pkgs.len(),
                aur_pkgs.len()
            );

            if !confirm_action(&msg)? {
                println!("Installation cancelled.");
                return Ok(());
            }

            if !repo_pkgs.is_empty() {
                let refs: Vec<&str> = repo_pkgs.iter().map(|s| s.as_str()).collect();
                install_package_names(PackageManager::Pacman, &refs)?;
            }

            if !aur_pkgs.is_empty() {
                if aur_helper.is_some() {
                    let refs: Vec<&str> = aur_pkgs.iter().map(|s| s.as_str()).collect();
                    install_package_names(PackageManager::Aur, &refs)?;
                } else {
                    println!("Warning: No AUR helper found. Skipping: {:?}", aur_pkgs);
                }
            }

            println!("✓ Package installation completed successfully!");
            Ok(())
        }
        FzfResult::Selected(row) => {
            let source_str = row.payload.manager.as_str();
            let name = row.payload.package.as_str();

            if debug {
                println!("Selected: {} ({})", name, source_str);
            }

            match source_str {
                src if src == PackageManager::Aur.as_str() => {
                    if aur_helper.is_some() {
                        install_package_names(PackageManager::Aur, &[name])?;
                    } else {
                        anyhow::bail!("AUR package selected but no AUR helper found");
                    }
                }
                _ => install_package_names(PackageManager::Pacman, &[name])?,
            }

            println!("✓ Package installation completed successfully!");
            Ok(())
        }
        FzfResult::MultiSelected(_) | FzfResult::Cancelled => {
            println!("No packages selected.");
            Ok(())
        }
        FzfResult::Error(err) => anyhow::bail!("Package selection failed: {}", err),
    }
}

/// Parse Arch selections into (repo_packages, aur_packages).
fn parse_arch_selections(
    rows: &[DecodedStreamingMenuItem<PackageSelectionPayload>],
) -> (Vec<String>, Vec<String>) {
    let mut repo = Vec::new();
    let mut aur = Vec::new();

    for row in rows {
        if row.payload.manager == PackageManager::Aur.as_str() {
            aur.push(row.payload.package.clone());
        } else {
            repo.push(row.payload.package.clone());
        }
    }

    (repo, aur)
}

// ============================================================================
// Shared Utilities
// ============================================================================

/// Handle install result for simple (non-Arch) package managers.
pub(crate) fn handle_install_result<F>(
    result: FzfResult<DecodedStreamingMenuItem<PackageSelectionPayload>>,
    install_fn: F,
    debug: bool,
) -> Result<()>
where
    F: FnOnce(&[&str]) -> Result<()>,
{
    match result {
        FzfResult::MultiSelected(rows) if !rows.is_empty() => {
            let packages: Vec<String> = rows
                .into_iter()
                .map(|row| row.payload.package)
                .filter(|s| !s.is_empty())
                .collect();

            if packages.is_empty() {
                println!("No valid packages selected.");
                return Ok(());
            }

            if debug {
                println!("Selected packages: {:?}", packages);
            }

            let msg = format!(
                "Install {} package{}?",
                packages.len(),
                if packages.len() == 1 { "" } else { "s" }
            );

            if !confirm_action(&msg)? {
                println!("Installation cancelled.");
                return Ok(());
            }

            let refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            install_fn(&refs)?;

            println!("✓ Package installation completed successfully!");
            Ok(())
        }
        FzfResult::Selected(row) => {
            let name = row.payload.package;

            if debug {
                println!("Selected package: {}", name);
            }
            install_fn(&[&name])?;
            println!("✓ Package installation completed successfully!");
            Ok(())
        }
        FzfResult::MultiSelected(_) | FzfResult::Cancelled => {
            println!("No packages selected.");
            Ok(())
        }
        FzfResult::Error(err) => anyhow::bail!("Package selection failed: {}", err),
    }
}

/// Show confirmation dialog and return whether user confirmed.
fn confirm_action(message: &str) -> Result<bool> {
    let result = FzfWrapper::builder().confirm(message).confirm_dialog()?;
    Ok(matches!(result, ConfirmResult::Yes))
}
