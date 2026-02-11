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

    let list_cmd = manager.list_available_command();
    let preview_id = preview_id_for_manager(manager);
    let preview_cmd = preview_command_streaming(preview_id);

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

/// Map a PackageManager to its corresponding PreviewId.
fn preview_id_for_manager(manager: PackageManager) -> PreviewId {
    match manager {
        PackageManager::Apt => PreviewId::Apt,
        PackageManager::Dnf => PreviewId::Dnf,
        PackageManager::Zypper => PreviewId::Zypper,
        PackageManager::Pacman => PreviewId::Pacman,
        PackageManager::Snap => PreviewId::Snap,
        PackageManager::Pkg => PreviewId::Pkg,
        PackageManager::Flatpak => PreviewId::Flatpak,
        PackageManager::Aur => PreviewId::Aur,
        PackageManager::Cargo => PreviewId::Cargo,
    }
}

/// Run the interactive Snap installer.
pub fn run_snap_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Snap package installer...");
    }

    if !PackageManager::Snap.is_available() {
        anyhow::bail!("Snap is not available on this system");
    }

    let preview_cmd = preview_command_streaming(PreviewId::Snap);

    // Build reload command that searches snaps as user types
    // Uses --phony to disable local filtering and rely on snap find results
    // Output format: name\tversion\tpublisher\tsummary
    let reload_cmd = "snap find '{q}' 2>/dev/null | awk 'NR>1 && !/^Name[[:space:]]+Version/ && !/Provide a search term/ && NF { \
        name = $1; version = $2; publisher = $3; summary = \"\"; \
        for(i=5; i<=NF; i++) summary = summary $i \" \"; \
        print name \"\\t\" version \"\\t\" publisher \"\\t\" summary \
    }' || true";

    // Load featured snaps on start; search as user types
    let featured_snaps = "snap find 2>/dev/null | awk 'NR>1 && !/^Name[[:space:]]+Version/ && !/Provide a search term/ && NF { \
        name = $1; version = $2; publisher = $3; summary = \"\"; \
        for(i=5; i<=NF; i++) summary = summary $i \" \"; \
        print name \"\\t\" version \"\\t\" publisher \"\\t\" summary \
    }'";

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Search snaps")
        .header(Header::fancy("Type to search Snap Store"))
        .args(fzf_mocha_args())
        .args([
            "--delimiter",
            "\t",
            "--with-nth",
            "1",
            "--preview",
            &preview_cmd,
            "--bind",
            &format!("change:reload:{}", reload_cmd),
            "--phony",
            "--ansi",
        ])
        .responsive_layout()
        .select_streaming(featured_snaps)
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

    // Build streaming command: source<TAB>package_name
    let mut list_cmds = Vec::new();
    if has_pacman {
        let cmd = PackageManager::Pacman.list_available_command();
        list_cmds.push(format!(
            "{} | sed 's/^/{}\\t/'",
            cmd,
            PackageManager::Pacman.as_str()
        ));
    }
    if aur_helper.is_some() {
        let cmd = PackageManager::Aur.list_available_command();
        list_cmds.push(format!(
            "{} | sed 's/^/{}\\t/'",
            cmd,
            PackageManager::Aur.as_str()
        ));
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
            let (source_str, name) = line
                .split_once('\t')
                .unwrap_or((PackageManager::Pacman.as_str(), &line));

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
fn parse_arch_selections(lines: &[String]) -> (Vec<String>, Vec<String>) {
    let mut repo = Vec::new();
    let mut aur = Vec::new();

    for line in lines {
        if let Some((source_str, name)) = line.split_once('\t') {
            if source_str == PackageManager::Aur.as_str() {
                aur.push(name.to_string());
            } else {
                repo.push(name.to_string());
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
pub(crate) fn handle_install_result<F>(
    result: FzfResult<String>,
    install_fn: F,
    debug: bool,
) -> Result<()>
where
    F: FnOnce(&[&str]) -> Result<()>,
{
    match result {
        FzfResult::MultiSelected(lines) if !lines.is_empty() => {
            let packages: Vec<String> = lines
                .into_iter()
                .map(|l| {
                    if let Some((_, rest)) = l.split_once('\t') {
                        rest.split_whitespace().next().unwrap_or(rest).to_string()
                    } else {
                        l.split_whitespace().next().unwrap_or(&l).to_string()
                    }
                })
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
            let name = if let Some((_, rest)) = line.split_once('\t') {
                rest.split_whitespace().next().unwrap_or(rest).to_string()
            } else {
                line.split_whitespace().next().unwrap_or(&line).to_string()
            };

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
