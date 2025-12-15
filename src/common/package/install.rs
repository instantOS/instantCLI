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

    match manager {
        PackageManager::Pacman => install_pacman(&package_names),
        PackageManager::Apt => install_apt(&package_names),
        PackageManager::Dnf => install_dnf(&package_names),
        PackageManager::Zypper => install_zypper(&package_names),
        PackageManager::Flatpak => install_flatpak(&package_names),
        PackageManager::Aur => install_aur(&package_names),
        PackageManager::Cargo => install_cargo(&package_names),
        PackageManager::Snap => install_snap(&package_names),
    }
}

/// Install packages using pacman.
fn install_pacman(packages: &[&str]) -> Result<()> {
    let mut args = vec!["pacman", "-S", "--noconfirm"];
    args.extend(packages);

    cmd("sudo", &args)
        .run()
        .context("Failed to install packages with pacman")?;

    Ok(())
}

/// Install packages using apt.
fn install_apt(packages: &[&str]) -> Result<()> {
    let mut args = vec!["apt", "install", "-y"];
    args.extend(packages);

    cmd("sudo", &args)
        .run()
        .context("Failed to install packages with apt")?;

    Ok(())
}

/// Install packages using dnf.
fn install_dnf(packages: &[&str]) -> Result<()> {
    let mut args = vec!["dnf", "install", "-y"];
    args.extend(packages);

    cmd("sudo", &args)
        .run()
        .context("Failed to install packages with dnf")?;

    Ok(())
}

/// Install packages using zypper.
fn install_zypper(packages: &[&str]) -> Result<()> {
    let mut args = vec!["zypper", "install", "-y"];
    args.extend(packages);

    cmd("sudo", &args)
        .run()
        .context("Failed to install packages with zypper")?;

    Ok(())
}

/// Install packages from Flatpak (Flathub).
fn install_flatpak(packages: &[&str]) -> Result<()> {
    // Ensure Flathub is configured
    if !is_flathub_configured() {
        setup_flathub()?;
    }

    let mut args = vec!["install", "-y", "flathub"];
    args.extend(packages);

    cmd("flatpak", &args)
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

    let mut args = vec!["-S", "--noconfirm"];
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
    let mut args = vec!["snap", "install"];
    args.extend(packages);

    cmd("sudo", &args)
        .run()
        .context("Failed to install packages with snap")?;

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
