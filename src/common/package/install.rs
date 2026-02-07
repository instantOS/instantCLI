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
        PackageManager::Pacman => install_pacman(packages),
        PackageManager::Apt => install_apt(packages),
        PackageManager::Dnf => install_dnf(packages),
        PackageManager::Zypper => install_zypper(packages),
        PackageManager::Flatpak => install_flatpak(packages),
        PackageManager::Aur => install_aur(packages),
        PackageManager::Cargo => install_cargo(packages),
        PackageManager::Snap => install_snap(packages),
        PackageManager::Pkg => install_pkg(packages),
    }
}

/// Install packages using pacman.
fn install_pacman(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Pacman.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to install packages with pacman")?;

    Ok(())
}

/// Install packages using apt.
fn install_apt(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Apt.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to install packages with apt")?;

    Ok(())
}

/// Install packages using dnf.
fn install_dnf(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Dnf.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to install packages with dnf")?;

    Ok(())
}

/// Install packages using zypper.
fn install_zypper(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Zypper.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to install packages with zypper")?;

    Ok(())
}

/// Install packages using pkg (Termux).
fn install_pkg(packages: &[&str]) -> Result<()> {
    let (program, base_args) = PackageManager::Pkg.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(program, &args)
        .run()
        .context("Failed to install packages with pkg")?;

    Ok(())
}

/// Install packages from Flatpak (Flathub).
fn install_flatpak(packages: &[&str]) -> Result<()> {
    // Ensure Flathub is configured
    if !is_flathub_configured() {
        setup_flathub()?;
    }

    let (program, base_args) = PackageManager::Flatpak.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(program, &args)
        .run()
        .context("Failed to install packages from Flatpak")?;

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

/// Install packages from AUR using the detected AUR helper.
fn install_aur(packages: &[&str]) -> Result<()> {
    let helper = detect_aur_helper()
        .ok_or_else(|| anyhow::anyhow!("No AUR helper found (install yay, paru, etc.)"))?;

    let (_default_helper, base_args) = PackageManager::Aur.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(helper, &args)
        .run()
        .with_context(|| format!("Failed to install packages with {}", helper))?;

    Ok(())
}

/// Install packages using cargo.
fn install_cargo(packages: &[&str]) -> Result<()> {
    // Cargo installs one package at a time, but we can batch them
    for package in packages {
        cmd("cargo", &["install", package])
            .run()
            .with_context(|| format!("Failed to install {} with cargo", package))?;
    }

    Ok(())
}

/// Install packages using snap.
fn install_snap(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Snap.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to install packages with snap")?;

    Ok(())
}

/// Uninstall packages using the specified package manager.
pub fn uninstall_packages(manager: PackageManager, packages: &[&str]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    match manager {
        PackageManager::Pacman => uninstall_pacman(packages),
        PackageManager::Apt => uninstall_apt(packages),
        PackageManager::Dnf => uninstall_dnf(packages),
        PackageManager::Zypper => uninstall_zypper(packages),
        PackageManager::Pkg => uninstall_pkg(packages),
        PackageManager::Flatpak => uninstall_flatpak(packages),
        PackageManager::Aur => uninstall_aur(packages),
        PackageManager::Snap => uninstall_snap(packages),
        PackageManager::Cargo => {
            anyhow::bail!("Cargo packages must be uninstalled manually")
        }
    }
}

/// Uninstall packages using pacman.
fn uninstall_pacman(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Pacman.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to uninstall packages with pacman")?;

    Ok(())
}

/// Uninstall packages using apt.
fn uninstall_apt(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Apt.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to uninstall packages with apt")?;

    Ok(())
}

/// Uninstall packages using dnf.
fn uninstall_dnf(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Dnf.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to uninstall packages with dnf")?;

    Ok(())
}

/// Uninstall packages using zypper.
fn uninstall_zypper(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Zypper.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to uninstall packages with zypper")?;

    Ok(())
}

/// Uninstall packages using pkg (Termux).
fn uninstall_pkg(packages: &[&str]) -> Result<()> {
    let (program, base_args) = PackageManager::Pkg.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(program, &args)
        .run()
        .context("Failed to uninstall packages with pkg")?;

    Ok(())
}

/// Uninstall packages from Flatpak.
fn uninstall_flatpak(packages: &[&str]) -> Result<()> {
    let (program, base_args) = PackageManager::Flatpak.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(program, &args)
        .run()
        .context("Failed to uninstall packages from Flatpak")?;

    Ok(())
}

/// Uninstall packages from AUR using the detected AUR helper.
fn uninstall_aur(packages: &[&str]) -> Result<()> {
    let helper = detect_aur_helper()
        .ok_or_else(|| anyhow::anyhow!("No AUR helper found (install yay, paru, etc.)"))?;

    let (_default_helper, base_args) = PackageManager::Aur.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(helper, &args)
        .run()
        .with_context(|| format!("Failed to uninstall packages with {}", helper))?;

    Ok(())
}

/// Uninstall packages using snap.
fn uninstall_snap(packages: &[&str]) -> Result<()> {
    let (sudo, base_args) = PackageManager::Snap.uninstall_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.extend(packages);

    cmd(sudo, &args)
        .run()
        .context("Failed to uninstall packages with snap")?;

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
