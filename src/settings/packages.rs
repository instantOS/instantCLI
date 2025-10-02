use anyhow::{Context, Result};
use std::process::Command;
use crate::fzf_wrapper::{FzfWrapper, FzfResult};
use super::SettingsContext;

/// Run the interactive package installer as a settings action
pub fn run_package_installer_action(ctx: &mut SettingsContext) -> Result<()> {
    run_package_installer(ctx.debug())
}

/// Run the interactive package installer
fn run_package_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting package installer...");
    }

    // Check if pacman is available
    if !is_pacman_available() {
        anyhow::bail!("pacman is not available on this system");
    }

    // Build the FZF selection with streaming input
    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages to install")
        .args([
            "--preview",
            "pacman -Sii {}",
            "--preview-window",
            "down:65%:wrap",
        ])
        .select_streaming("pacman -Slq")
        .context("Failed to run package selector")?;

    match result {
        FzfResult::MultiSelected(packages) => {
            if packages.is_empty() {
                println!("No packages selected.");
                return Ok(());
            }

            if debug {
                println!("Selected packages: {:?}", packages);
            }

            // Confirm installation
            let confirm_msg = format!(
                "Install {} package{}?",
                packages.len(),
                if packages.len() == 1 { "" } else { "s" }
            );

            let confirm = FzfWrapper::builder()
                .confirm(&confirm_msg)
                .show_confirmation()?;

            if !matches!(confirm, crate::fzf_wrapper::ConfirmResult::Yes) {
                println!("Installation cancelled.");
                return Ok(());
            }

            // Install packages
            install_packages(&packages, debug)?;
            
            // Show success message
            println!("✓ Package installation completed successfully!");
        }
        FzfResult::Selected(package) => {
            // Single selection (shouldn't happen with multi-select, but handle it)
            if debug {
                println!("Selected package: {}", package);
            }
            install_packages(&[package], debug)?;
            println!("✓ Package installation completed successfully!");
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

/// Check if pacman is available on the system
fn is_pacman_available() -> bool {
    Command::new("which")
        .arg("pacman")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Install packages using pacman
fn install_packages(packages: &[String], debug: bool) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    if debug {
        println!("Installing packages: {}", packages.join(" "));
    }

    println!("Installing packages...");
    
    let status = Command::new("sudo")
        .arg("pacman")
        .arg("-S")
        .arg("--noconfirm")
        .args(packages)
        .status()
        .context("Failed to execute pacman")?;

    if !status.success() {
        anyhow::bail!("Package installation failed");
    }

    Ok(())
}
