//! Package installation backends for each package manager.

use anyhow::{Context, Result};
use duct::cmd;

use super::PackageManager;
use super::batch::PackageToInstall;
use super::manager::detect_aur_helper;

/// Install packages using the specified package manager.
pub fn install_packages(manager: PackageManager, packages: &[PackageToInstall]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let package_names: Vec<&str> = packages
        .iter()
        .map(|p| p.package_def.package_name)
        .collect();

    install_package_names(manager, &package_names)
}

/// Install packages by name using the specified package manager.
///
/// This is a simpler interface for interactive UIs that work with raw package names
/// rather than `PackageToInstall` structs.
pub fn install_package_names(manager: PackageManager, packages: &[&str]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    match manager {
        // Special case: Flatpak needs Flathub setup
        PackageManager::Flatpak => {
            if !is_flathub_configured() {
                setup_flathub()?;
            }
            install_generic(manager, packages)
        }

        // Special case: AUR needs helper detection
        PackageManager::Aur => {
            let helper = detect_aur_helper()
                .ok_or_else(|| anyhow::anyhow!("No AUR helper found (install yay, paru, etc.)"))?;
            install_aur_with_helper(helper, packages)
        }

        // Special case: Cargo installs one package at a time
        PackageManager::Cargo => {
            for package in packages {
                cmd("cargo", &["install", package])
                    .run()
                    .with_context(|| format!("Failed to install {} with cargo", package))?;
            }
            Ok(())
        }

        // All other package managers use the generic pattern
        _ => install_generic(manager, packages),
    }
}

/// Generic install function for package managers with standard command patterns.
///
/// This handles all package managers that follow the pattern:
/// `<program> <base_args> <package1> <package2> ...`
fn install_generic(manager: PackageManager, packages: &[&str]) -> Result<()> {
    let (program, base_args) = manager.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(program, &args)
        .run()
        .with_context(|| format!("Failed to install packages with {}", manager.display_name()))?;

    Ok(())
}

/// Install packages from AUR using a specific helper (yay, paru, etc.).
fn install_aur_with_helper(helper: &str, packages: &[&str]) -> Result<()> {
    let (_default_helper, base_args) = PackageManager::Aur.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(helper, &args)
        .run()
        .with_context(|| format!("Failed to install packages with {}", helper))?;

    Ok(())
}

/// Check if Flathub remote is configured.
fn is_flathub_configured() -> bool {
    cmd!("flatpak", "remotes", "--columns=name")
        .read()
        .map(|output| output.lines().any(|line| line.trim() == "flathub"))
        .unwrap_or(false)
}

/// Set up Flathub remote.
fn setup_flathub() -> Result<()> {
    cmd!(
        "flatpak",
        "remote-add",
        "--if-not-exists",
        "flathub",
        "https://flathub.org/repo/flathub.flatpakrepo"
    )
    .run()
    .context("Failed to add Flathub remote")?;

    Ok(())
}

/// Uninstall packages using the specified package manager.
pub fn uninstall_packages(manager: PackageManager, packages: &[&str]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    match manager {
        // Special case: AUR needs helper detection
        PackageManager::Aur => {
            let helper = detect_aur_helper()
                .ok_or_else(|| anyhow::anyhow!("No AUR helper found (install yay, paru, etc.)"))?;
            uninstall_aur_with_helper(helper, packages)
        }

        // All other package managers use the generic pattern (including Cargo)
        _ => uninstall_generic(manager, packages),
    }
}

/// Generic uninstall function for package managers with standard command patterns.
///
/// This handles all package managers that follow the pattern:
/// `<program> <base_args> <package1> <package2> ...`
fn uninstall_generic(manager: PackageManager, packages: &[&str]) -> Result<()> {
    let (program, base_args) = manager.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(program, &args).run().with_context(|| {
        format!(
            "Failed to uninstall packages with {}",
            manager.display_name()
        )
    })?;

    Ok(())
}

/// Uninstall packages from AUR using a specific helper (yay, paru, etc.).
fn uninstall_aur_with_helper(helper: &str, packages: &[&str]) -> Result<()> {
    let (_default_helper, base_args) = PackageManager::Aur.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(helper, &args)
        .run()
        .with_context(|| format!("Failed to uninstall packages with {}", helper))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_flathub_configured_on_system_without_flatpak() {
        // This test just ensures the function doesn't panic
        // The actual result depends on the system
        let _ = is_flathub_configured();
    }
}
