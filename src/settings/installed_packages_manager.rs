//! Installed packages manager
//!
//! Interactive package browser and uninstaller for installed system packages.

use anyhow::{Context, Result};

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, uninstall_packages};
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

    let termux = is_termux();
    let has_apt = is_apt_available();
    let has_pkg = is_pkg_available();

    // Validate package manager availability
    if termux {
        if !has_pkg {
            anyhow::bail!("pkg is not available on this Termux system");
        }
    } else if !has_apt {
        anyhow::bail!("apt is not available on this system");
    }

    // List installed packages (one package per line)
    let list_cmd = "dpkg-query -W -f='${Package}\n' 2>/dev/null | sort";

    // Preview: apt show <package> (each line is just a package name)
    let preview_cmd = "apt show {1} 2>/dev/null";

    // FZF prompt customization
    let prompt = if termux {
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
            preview_cmd,
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

            let manager = if is_termux() {
                PackageManager::Pkg
            } else {
                PackageManager::Apt
            };
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

            let manager = if is_termux() {
                PackageManager::Pkg
            } else {
                PackageManager::Apt
            };
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

/// Check if apt is available on the system
fn is_apt_available() -> bool {
    which::which("apt").is_ok()
}

/// Check if running on Termux
fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

/// Check if pkg (Termux package manager) is available
fn is_pkg_available() -> bool {
    which::which("pkg").is_ok()
}

// ============================================================================
// Arch Package Manager
// ============================================================================

/// Run the Arch Linux installed packages manager
fn run_arch_package_manager(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Arch package manager...");
    }

    let aur_helper = detect_aur_helper();
    let has_pacman = is_pacman_available();

    if !has_pacman && aur_helper.is_none() {
        anyhow::bail!("Neither pacman nor an AUR helper is available on this system");
    }

    // List installed packages (one package per line)
    let list_cmd = "pacman -Qq | sort";

    // Preview: pacman -Qi <package> (each line is just a package name)
    let preview_cmd = "pacman -Qi {1}";

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages to uninstall")
        .header("Tab to select multiple packages, Enter to confirm uninstall")
        .responsive_layout()
        .args([
            "--preview",
            preview_cmd,
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

/// Detect available AUR helper (yay, paru, etc.)
fn detect_aur_helper() -> Option<String> {
    const AUR_HELPERS: &[&str] = &["yay", "paru", "pikaur", "trizen"];

    AUR_HELPERS
        .iter()
        .find(|&&helper| which::which(helper).is_ok())
        .map(|s| s.to_string())
}

/// Check if pacman is available on the system
fn is_pacman_available() -> bool {
    which::which("pacman").is_ok()
}
