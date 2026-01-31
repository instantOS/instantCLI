//! Installed packages manager
//!
//! Interactive package browser and uninstaller for installed system packages.

use anyhow::{Context, Result};

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, detect_aur_helper, uninstall_packages};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper};

/// Run the installed packages manager
pub fn run_installed_packages_manager(debug: bool) -> Result<()> {
    let os = OperatingSystem::detect();

    if os.is_debian_based() {
        run_debian_package_manager(debug)
    } else if os.is_arch_based() {
        run_arch_package_manager(debug)
    } else {
        anyhow::bail!(
            "Package manager not supported on this system ({})",
            os.name()
        )
    }
}

// ============================================================================
// Debian/Ubuntu Package Manager
// ============================================================================

/// Run the Debian/Ubuntu installed packages manager
fn run_debian_package_manager(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Debian package manager...");
    }

    let os = OperatingSystem::detect();
    let is_termux = matches!(os, OperatingSystem::Termux);

    // Validate package manager availability
    let manager = if is_termux {
        PackageManager::Pkg
    } else {
        PackageManager::Apt
    };

    if !manager.is_available() {
        anyhow::bail!("{} is not available on this system", manager);
    }

    // List installed packages (one package per line)
    let list_cmd = manager.list_installed_command();

    // Preview command
    let preview_cmd = manager.show_package_command().replace("{package}", "{1}");

    // FZF prompt customization
    let prompt = if is_termux {
        "Select Termux packages to uninstall"
    } else {
        "Select packages to uninstall"
    };

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt(prompt)
        .header("Tab to select multiple packages, Enter to confirm uninstall")
        .responsive_layout()
        .args([
            "--preview",
            preview_cmd.as_str(),
            "--preview-window",
            "down:40%:wrap", // Smaller preview for more item space
            "--layout",
            "reverse-list", // More compact, dense layout for many items
            "--height",
            "90%", // Use most of the screen
            "--bind",
            "ctrl-l:clear-screen",
            "--ansi",
            "--no-mouse",
        ])
        .select_streaming(list_cmd)
        .context("Failed to run package selector")?;

    match result {
        FzfResult::MultiSelected(lines) => {
            if lines.is_empty() {
                println!("No packages selected.");
                return Ok(());
            }

            let packages: Vec<String> = lines.into_iter().map(|s| s.to_string()).collect();

            if debug {
                println!("Selected packages: {:?}", packages);
            }

            // Confirm uninstallation
            let confirm_msg = format!(
                "Uninstall {} package{}?\n\nThis action cannot be undone.",
                packages.len(),
                if packages.len() == 1 { "" } else { "s" }
            );

            let confirm = FzfWrapper::builder()
                .confirm(&confirm_msg)
                .yes_text("Uninstall")
                .no_text("Cancel")
                .show_confirmation()?;

            if !matches!(confirm, ConfirmResult::Yes) {
                println!("Uninstall cancelled.");
                return Ok(());
            }

            let refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            uninstall_packages(manager, &refs)?;

            println!("✓ Package uninstallation completed successfully!");
        }
        FzfResult::Selected(line) => {
            let package_name = line.trim().to_string();

            if debug {
                println!("Selected package: {}", package_name);
            }

            // Confirm uninstallation
            let confirm_msg = format!(
                "Uninstall package '{}'?\n\nThis action cannot be undone.",
                package_name
            );

            let confirm = FzfWrapper::builder()
                .confirm(&confirm_msg)
                .yes_text("Uninstall")
                .no_text("Cancel")
                .show_confirmation()?;

            if !matches!(confirm, ConfirmResult::Yes) {
                println!("Uninstall cancelled.");
                return Ok(());
            }

            uninstall_packages(manager, &[&package_name])?;

            println!("✓ Package uninstallation completed successfully!");
        }
        FzfResult::Cancelled => {
            println!("Package selection cancelled.");
        }
        FzfResult::Error(err) => {
            anyhow::bail!("Package selection failed: {}", err);
        }
    }

    Ok(())
}

// ============================================================================
// Arch Package Manager
// ============================================================================

/// Run the Arch Linux installed packages manager
fn run_arch_package_manager(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Arch package manager...");
    }

    let has_pacman = PackageManager::Pacman.is_available();
    let has_aur = detect_aur_helper().is_some();

    if !has_pacman && !has_aur {
        anyhow::bail!("Neither pacman nor an AUR helper is available on this system");
    }

    // List installed packages (one package per line)
    let list_cmd = PackageManager::Pacman.list_installed_command();

    // Preview command
    let preview_cmd = PackageManager::Pacman
        .show_package_command()
        .replace("{package}", "{1}");

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages to uninstall")
        .header("Tab to select multiple packages, Enter to confirm uninstall")
        .responsive_layout()
        .args([
            "--preview",
            preview_cmd.as_str(),
            "--preview-window",
            "down:40%:wrap", // Smaller preview for more item space
            "--layout",
            "reverse-list", // More compact, dense layout for many items
            "--height",
            "90%", // Use most of the screen
            "--bind",
            "ctrl-l:clear-screen",
            "--ansi",
            "--no-mouse",
        ])
        .select_streaming(list_cmd)
        .context("Failed to run package selector")?;

    match result {
        FzfResult::MultiSelected(lines) => {
            if lines.is_empty() {
                println!("No packages selected.");
                return Ok(());
            }

            let packages: Vec<String> = lines.into_iter().map(|s| s.to_string()).collect();

            if debug {
                println!("Selected packages: {:?}", packages);
            }

            // Confirm uninstallation
            let confirm_msg = format!(
                "Uninstall {} package{}?\n\nThis action cannot be undone.",
                packages.len(),
                if packages.len() == 1 { "" } else { "s" }
            );

            let confirm = FzfWrapper::builder()
                .confirm(&confirm_msg)
                .yes_text("Uninstall")
                .no_text("Cancel")
                .show_confirmation()?;

            if !matches!(confirm, ConfirmResult::Yes) {
                println!("Uninstall cancelled.");
                return Ok(());
            }

            let refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            uninstall_packages(PackageManager::Pacman, &refs)?;

            println!("✓ Package uninstallation completed successfully!");
        }
        FzfResult::Selected(line) => {
            let package_name = line.trim().to_string();

            if debug {
                println!("Selected package: {}", package_name);
            }

            // Confirm uninstallation
            let confirm_msg = format!(
                "Uninstall package '{}'?\n\nThis action cannot be undone.",
                package_name
            );

            let confirm = FzfWrapper::builder()
                .confirm(&confirm_msg)
                .yes_text("Uninstall")
                .no_text("Cancel")
                .show_confirmation()?;

            if !matches!(confirm, ConfirmResult::Yes) {
                println!("Uninstall cancelled.");
                return Ok(());
            }

            uninstall_packages(PackageManager::Pacman, &[&package_name])?;

            println!("✓ Package uninstallation completed successfully!");
        }
        FzfResult::Cancelled => {
            println!("Package selection cancelled.");
        }
        FzfResult::Error(err) => {
            anyhow::bail!("Package selection failed: {}", err);
        }
    }

    Ok(())
}
