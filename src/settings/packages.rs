use super::SettingsContext;
use crate::common::distro::OperatingSystem;
use crate::menu_utils::{FzfResult, FzfWrapper};
use anyhow::{Context, Result};
use std::process::Command;

/// Run the interactive package installer as a settings action
/// This dispatches to the appropriate package manager based on the detected OS.
pub fn run_package_installer_action(ctx: &mut SettingsContext) -> Result<()> {
    let os = OperatingSystem::detect();

    if os.is_arch_based() {
        run_unified_package_installer(ctx.debug())
    } else if os.is_debian_based() {
        run_debian_package_installer(ctx.debug())
    } else {
        anyhow::bail!(
            "Package installer not supported on this system ({})",
            os.name()
        )
    }
}

/// Package source enumeration
#[derive(Debug, Clone, PartialEq)]
enum PackageSource {
    Pacman,
    AUR,
}

/// A package with its source information
#[derive(Debug, Clone)]
struct Package {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    source: PackageSource,
}

impl Package {
    fn extract_name_from_display(display: &str) -> (String, PackageSource) {
        if display.starts_with("[AUR] ") {
            (display[6..].to_string(), PackageSource::AUR)
        } else if display.starts_with("[Repo] ") {
            (display[7..].to_string(), PackageSource::Pacman)
        } else {
            // Fallback for backwards compatibility
            (display.to_string(), PackageSource::Pacman)
        }
    }
}

/// Run the unified package installer (supports both pacman and AUR)
fn run_unified_package_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting unified package installer...");
    }

    // Check what package managers are available
    let aur_helper = detect_aur_helper();
    let has_pacman = is_pacman_available();

    if !has_pacman && aur_helper.is_none() {
        anyhow::bail!("Neither pacman nor an AUR helper is available on this system");
    }

    // Construct the list command
    let mut list_cmds = Vec::new();

    if has_pacman {
        // List repo packages
        list_cmds.push("pacman -Slq | sed 's/^/[Repo] /'");
    }

    if aur_helper.is_some() {
        // List AUR packages using the static list
        // We use curl to fetch the list because helpers like yay don't easily list ONLY AUR packages
        list_cmds.push("curl -sL https://aur.archlinux.org/packages.gz 2>/dev/null | gunzip 2>/dev/null | sed 's/^/[AUR] /'");
    }

    // Combine commands to run in sequence
    let full_command = format!("{{ {}; }}", list_cmds.join("; "));

    // Determine preview command
    // If we have an AUR helper, it can usually preview both repo and AUR packages
    let preview_cmd = if let Some(ref helper) = aur_helper {
        format!("{} -Sii {{2}}", helper)
    } else {
        "pacman -Sii {2}".to_string()
    };

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages to install")
        .responsive_layout()
        .args([
            "--preview",
            &preview_cmd,
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
            "--delimiter",
            " ",
            "--with-nth",
            "1..",
        ])
        .select_streaming(&full_command)
        .context("Failed to run package selector")?;

    match result {
        FzfResult::MultiSelected(lines) => {
            if lines.is_empty() {
                println!("No packages selected.");
                return Ok(());
            }

            let mut pacman_packages = Vec::new();
            let mut aur_packages = Vec::new();

            for line in lines {
                let (name, source) = Package::extract_name_from_display(&line);
                match source {
                    PackageSource::Pacman => pacman_packages.push(name),
                    PackageSource::AUR => aur_packages.push(name),
                }
            }

            if debug {
                println!("Selected Repo packages: {:?}", pacman_packages);
                println!("Selected AUR packages: {:?}", aur_packages);
            }

            // Confirm installation
            let total = pacman_packages.len() + aur_packages.len();
            let confirm_msg = format!(
                "Install {} package{} ({} Repo, {} AUR)?",
                total,
                if total == 1 { "" } else { "s" },
                pacman_packages.len(),
                aur_packages.len()
            );

            let confirm = FzfWrapper::builder()
                .confirm(&confirm_msg)
                .show_confirmation()?;

            if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
                println!("Installation cancelled.");
                return Ok(());
            }

            // Install Repo packages first
            if !pacman_packages.is_empty() {
                install_pacman_packages(&pacman_packages, debug)?;
            }

            // Install AUR packages
            if !aur_packages.is_empty() {
                if let Some(helper) = aur_helper {
                    install_aur_packages(&aur_packages, &helper, debug)?;
                } else {
                    println!(
                        "Warning: AUR packages selected but no AUR helper found. Skipping: {:?}",
                        aur_packages
                    );
                }
            }

            println!("✓ Package installation completed successfully!");
        }
        FzfResult::Selected(line) => {
            let (name, source) = Package::extract_name_from_display(&line);

            if debug {
                println!("Selected package: {} ({:?})", name, source);
            }

            match source {
                PackageSource::Pacman => install_pacman_packages(&[name], debug)?,
                PackageSource::AUR => {
                    if let Some(helper) = aur_helper {
                        install_aur_packages(&[name], &helper, debug)?;
                    } else {
                        println!("Warning: AUR package selected but no AUR helper found.");
                    }
                }
            }
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

/// Detect available AUR helper (yay, paru, etc.)
fn detect_aur_helper() -> Option<String> {
    const AUR_HELPERS: &[&str] = &["yay", "paru", "pikaur", "trizen"];

    AUR_HELPERS
        .iter()
        .find(|&&helper| which::which(helper).is_ok())
        .map(|&s| s.to_string())
}

/// Check if pacman is available on the system
fn is_pacman_available() -> bool {
    which::which("pacman").is_ok()
}

/// Install pacman packages
fn install_pacman_packages(packages: &[String], debug: bool) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    if debug {
        println!("Installing pacman packages: {}", packages.join(" "));
    }

    println!("Installing repository packages...");

    let status = Command::new("sudo")
        .arg("pacman")
        .arg("-S")
        .arg("--noconfirm")
        .args(packages)
        .status()
        .context("Failed to execute pacman")?;

    if !status.success() {
        anyhow::bail!("Pacman package installation failed");
    }

    Ok(())
}

/// Install AUR packages using the available AUR helper
fn install_aur_packages(packages: &[String], aur_helper: &str, debug: bool) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    if debug {
        println!(
            "Installing AUR packages with {}: {}",
            aur_helper,
            packages.join(" ")
        );
    }

    println!("Installing AUR packages...");

    let status = Command::new(aur_helper)
        .arg("-S")
        .arg("--noconfirm")
        .args(packages)
        .status()
        .context(format!("Failed to execute {}", aur_helper))?;

    if !status.success() {
        anyhow::bail!("AUR package installation failed");
    }

    Ok(())
}

// ============================================================================
// Debian/Ubuntu Package Installer
// ============================================================================

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

/// Install apt/pkg packages
fn install_apt_packages(packages: &[String], debug: bool) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    if debug {
        println!("Installing apt packages: {}", packages.join(" "));
    }

    let is_termux = is_termux();
    println!(
        "Installing {}packages...",
        if is_termux { "" } else { "repository " }
    );

    let status = if is_termux {
        // Termux: no sudo needed, use pkg
        Command::new("pkg")
            .arg("install")
            .arg("-y")
            .args(packages)
            .status()
            .context("Failed to execute pkg")?
    } else {
        // Debian/Ubuntu: use sudo apt
        Command::new("sudo")
            .arg("apt")
            .arg("install")
            .arg("-y")
            .args(packages)
            .status()
            .context("Failed to execute apt")?
    };

    if !status.success() {
        anyhow::bail!("Package installation failed");
    }

    Ok(())
}

/// Run the unified Debian/Ubuntu package installer
fn run_debian_package_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Debian package installer...");
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

    // Construct the list command - only package names, descriptions in preview
    // Extract just the package name from apt-cache search output (one per line)
    let list_cmd = "apt-cache search . 2>/dev/null | grep -v '^$' | cut -d' ' -f1";

    // Preview command: apt show works on both
    // {1} refers to the package name
    let preview_cmd = "apt show {1} 2>/dev/null";

    // FZF prompt customization
    let prompt = if termux {
        "Select Termux packages to install"
    } else {
        "Select packages to install"
    };

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt(prompt)
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

            // Parse package names - each line is just a package name
            let packages: Vec<String> = lines
                .into_iter()
                .map(|line| line.trim().to_string())
                .collect();

            if packages.is_empty() {
                println!("No valid packages selected.");
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

            if !matches!(confirm, crate::menu_utils::ConfirmResult::Yes) {
                println!("Installation cancelled.");
                return Ok(());
            }

            install_apt_packages(&packages, debug)?;

            println!("✓ Package installation completed successfully!");
        }
        FzfResult::Selected(line) => {
            let package_name = line.trim().to_string();

            if debug {
                println!("Selected package: {}", package_name);
            }

            install_apt_packages(&[package_name], debug)?;

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
