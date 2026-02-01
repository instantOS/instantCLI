use super::SettingsContext;
use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, detect_aur_helper, install_package_names};
use crate::menu_utils::{FzfResult, FzfWrapper};
use anyhow::{Context, Result};

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
    let has_pacman = PackageManager::Pacman.is_available();

    if !has_pacman && aur_helper.is_none() {
        anyhow::bail!("Neither pacman nor an AUR helper is available on this system");
    }

    // Construct the list command
    let mut list_cmds = Vec::new();

    if has_pacman {
        // List repo packages
        let pacman_list = PackageManager::Pacman.list_available_command();
        list_cmds.push(format!("{} | sed 's/^/[Repo] /'", pacman_list));
    }

    if aur_helper.is_some() {
        // List AUR packages using the static list
        // We use curl to fetch the list because helpers like yay don't easily list ONLY AUR packages
        let aur_list = PackageManager::Aur.list_available_command();
        list_cmds.push(format!("{} | sed 's/^/[AUR] /'", aur_list));
    }

    // Combine commands to run in sequence
    let full_command = format!("{{ {}; }}", list_cmds.join("; "));

    // Determine preview command
    // If we have an AUR helper, it can usually preview both repo and AUR packages
    let preview_cmd = if let Some(helper) = aur_helper {
        format!("{} -Sii {{2}}", helper)
    } else {
        let pacman_show = PackageManager::Pacman.show_package_command();
        pacman_show.replace("{package}", "{2}")
    };

    // Re-detect for later use (consumed above)
    let aur_helper = detect_aur_helper();

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages to install")
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
                let refs: Vec<&str> = pacman_packages.iter().map(|s| s.as_str()).collect();
                install_package_names(PackageManager::Pacman, &refs)?;
            }

            // Install AUR packages
            if !aur_packages.is_empty() {
                if aur_helper.is_some() {
                    let refs: Vec<&str> = aur_packages.iter().map(|s| s.as_str()).collect();
                    install_package_names(PackageManager::Aur, &refs)?;
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
                PackageSource::Pacman => install_package_names(PackageManager::Pacman, &[&name])?,
                PackageSource::AUR => {
                    if aur_helper.is_some() {
                        install_package_names(PackageManager::Aur, &[&name])?;
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

// ============================================================================
// Debian/Ubuntu Package Installer
// ============================================================================

/// Run the unified Debian/Ubuntu package installer
fn run_debian_package_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Debian package installer...");
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

    // Construct the list command - only package names, descriptions in preview
    let list_cmd = manager.list_available_command();

    // Preview command
    let preview_cmd = manager.show_package_command().replace("{package}", "{1}");

    // FZF prompt customization
    let prompt = if is_termux {
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

            let refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            install_package_names(manager, &refs)?;

            println!("✓ Package installation completed successfully!");
        }
        FzfResult::Selected(line) => {
            let package_name = line.trim().to_string();

            if debug {
                println!("Selected package: {}", package_name);
            }

            install_package_names(manager, &[&package_name])?;

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
