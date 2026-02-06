//! Package installer for the settings TUI
//!
//! Provides interactive package installation using fzf with streaming for performance.

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, detect_aur_helper, install_package_names};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper, Header};
use crate::preview::{PreviewId, preview_command_streaming};
use crate::ui::catppuccin::fzf_mocha_args;
use anyhow::{Context, Result};

use super::SettingsContext;

/// Run the interactive package installer as a settings action.
/// Dispatches to the appropriate package manager based on the detected OS.
pub fn run_package_installer_action(ctx: &mut SettingsContext) -> Result<()> {
    let os = OperatingSystem::detect();
    let debug = ctx.debug();

    if os.is_arch_based() {
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

    let list_cmd = manager.list_available_command();
    let preview_cmd = preview_command_streaming(PreviewId::Package);

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages")
        .header(Header::fancy("Install Packages"))
        .args(fzf_mocha_args())
        .args(["--preview", &preview_cmd, "--ansi"])
        .responsive_layout()
        .select_streaming(list_cmd)
        .context("Failed to run package selector")?;

    handle_install_result(
        result,
        |packages| install_package_names(manager, packages),
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

    // Build streaming command: source<TAB>package_name
    let mut list_cmds = Vec::new();
    if has_pacman {
        let cmd = PackageManager::Pacman.list_available_command();
        list_cmds.push(format!("{} | sed 's/^/repo\\t/'", cmd));
    }
    if aur_helper.is_some() {
        let cmd = PackageManager::Aur.list_available_command();
        list_cmds.push(format!("{} | sed 's/^/aur\\t/'", cmd));
    }
    let full_command = format!("{{ {}; }}", list_cmds.join("; "));

    let preview_cmd = preview_command_streaming(PreviewId::Package);

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages")
        .header(Header::fancy("Install Packages"))
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
        .select_streaming(&full_command)
        .context("Failed to run package selector")?;

    handle_arch_install_result(result, detect_aur_helper(), debug)
}

/// Handle Arch install result, splitting packages by source.
fn handle_arch_install_result(
    result: FzfResult<String>,
    aur_helper: Option<&str>,
    debug: bool,
) -> Result<()> {
    match result {
        FzfResult::MultiSelected(lines) if !lines.is_empty() => {
            let (repo_pkgs, aur_pkgs) = parse_arch_selections(&lines);

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
        FzfResult::Selected(line) => {
            let (source, name) = line.split_once('\t').unwrap_or(("repo", &line));

            if debug {
                println!("Selected: {} ({})", name, source);
            }

            if source == "aur" {
                if aur_helper.is_some() {
                    install_package_names(PackageManager::Aur, &[name])?;
                } else {
                    anyhow::bail!("AUR package selected but no AUR helper found");
                }
            } else {
                install_package_names(PackageManager::Pacman, &[name])?;
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
fn parse_arch_selections(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut repo = Vec::new();
    let mut aur = Vec::new();

    for line in lines {
        if let Some((source, name)) = line.split_once('\t') {
            match source {
                "aur" => aur.push(name.to_string()),
                _ => repo.push(name.to_string()),
            }
        } else {
            repo.push(line.clone());
        }
    }

    (repo, aur)
}

// ============================================================================
// Shared Utilities
// ============================================================================

/// Handle install result for simple (non-Arch) package managers.
fn handle_install_result<F>(result: FzfResult<String>, install_fn: F, debug: bool) -> Result<()>
where
    F: FnOnce(&[&str]) -> Result<()>,
{
    match result {
        FzfResult::MultiSelected(lines) if !lines.is_empty() => {
            let packages: Vec<String> = lines
                .into_iter()
                .map(|l| l.trim().to_string())
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
        FzfResult::Selected(line) => {
            let name = line.trim();
            if debug {
                println!("Selected package: {}", name);
            }
            install_fn(&[name])?;
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
    let result = FzfWrapper::builder().confirm(message).show_confirmation()?;
    Ok(matches!(result, ConfirmResult::Yes))
}
