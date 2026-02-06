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
    } else if os.is_fedora_based() {
        run_fedora_package_installer(ctx.debug())
    } else {
        anyhow::bail!(
            "Package installer not supported on this system ({})",
            os.name()
        )
    }
}

// ============================================================================
// Arch Package Installer
// ============================================================================

/// Run the unified package installer (supports both pacman and AUR)
///
/// Uses a streaming approach for performance with large package lists.
/// Packages are displayed by name only; the preview shows source and details.
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

    // Build the streaming command that outputs: source<TAB>package_name
    // We use a tab delimiter so fzf can show only field 2 (package name)
    // while we use field 1 (source) in the preview
    let mut list_cmds = Vec::new();

    if has_pacman {
        // List repo packages with "repo" prefix
        let pacman_list = PackageManager::Pacman.list_available_command();
        list_cmds.push(format!("{} | sed 's/^/repo\\t/'", pacman_list));
    }

    if aur_helper.is_some() {
        // List AUR packages with "aur" prefix
        let aur_list = PackageManager::Aur.list_available_command();
        list_cmds.push(format!("{} | sed 's/^/aur\\t/'", aur_list));
    }

    // Combine commands
    let full_command = format!("{{ {}; }}", list_cmds.join("; "));

    // Build preview command that shows source info and package details
    // Field 1 = source (repo/aur), Field 2 = package name
    let preview_cmd = if let Some(helper) = aur_helper {
        // AUR helper can query both repo and AUR packages
        format!(
            r#"source=$(echo {{}} | cut -f1); pkg=$(echo {{}} | cut -f2);
if [ "$source" = "aur" ]; then
    echo -e "\033[38;2;203;166;247mAUR Package\033[0m"
    echo "────────────────────────────"
    {} -Si "$pkg" 2>/dev/null || echo "No info available"
else
    echo -e "\033[38;2;166;227;161mRepository Package\033[0m"
    echo "────────────────────────────"
    pacman -Si "$pkg" 2>/dev/null || echo "No info available"
fi"#,
            helper
        )
    } else {
        // Pacman only
        r#"source=$(echo {} | cut -f1); pkg=$(echo {} | cut -f2);
echo -e "\033[38;2;166;227;161mRepository Package\033[0m"
echo "────────────────────────────"
pacman -Si "$pkg" 2>/dev/null || echo "No info available""#
            .to_string()
    };

    // Re-detect for later use
    let aur_helper = detect_aur_helper();

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages to install")
        .header("Tab to select multiple, Enter to confirm")
        .responsive_layout()
        .args([
            "--delimiter",
            "\t",
            "--with-nth",
            "2", // Show only the package name (field 2)
            "--preview",
            preview_cmd.as_str(),
            "--preview-window",
            "down:40%:wrap",
            "--layout",
            "reverse-list",
            "--height",
            "90%",
            "--bind",
            "ctrl-l:clear-screen",
            "--ansi",
            "--no-mouse",
        ])
        .select_streaming(&full_command)
        .context("Failed to run package selector")?;

    match result {
        FzfResult::MultiSelected(lines) => {
            if lines.is_empty() {
                println!("No packages selected.");
                return Ok(());
            }

            // Parse selected lines: source<TAB>package_name
            let mut pacman_packages = Vec::new();
            let mut aur_packages = Vec::new();

            for line in &lines {
                if let Some((source, name)) = line.split_once('\t') {
                    match source {
                        "repo" => pacman_packages.push(name.to_string()),
                        "aur" => aur_packages.push(name.to_string()),
                        _ => pacman_packages.push(name.to_string()),
                    }
                } else {
                    // Fallback if no tab delimiter (shouldn't happen)
                    pacman_packages.push(line.clone());
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
            // Parse: source<TAB>package_name
            let (source, name) = line
                .split_once('\t')
                .map(|(s, n)| (s, n.to_string()))
                .unwrap_or(("repo", line.clone()));

            if debug {
                println!("Selected package: {} (source: {})", name, source);
            }

            match source {
                "aur" => {
                    if aur_helper.is_some() {
                        install_package_names(PackageManager::Aur, &[&name])?;
                    } else {
                        println!("Warning: AUR package selected but no AUR helper found.");
                        return Ok(());
                    }
                }
                _ => install_package_names(PackageManager::Pacman, &[&name])?,
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
        .header("Tab to select multiple, Enter to confirm")
        .responsive_layout()
        .args([
            "--preview",
            preview_cmd.as_str(),
            "--preview-window",
            "down:40%:wrap",
            "--layout",
            "reverse-list",
            "--height",
            "90%",
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

// ============================================================================
// Fedora Package Installer
// ============================================================================

/// Run the Fedora package installer (dnf)
fn run_fedora_package_installer(debug: bool) -> Result<()> {
    if debug {
        println!("Starting Fedora package installer...");
    }

    // Validate package manager availability
    let manager = PackageManager::Dnf;

    if !manager.is_available() {
        anyhow::bail!("{} is not available on this system", manager);
    }

    // Construct the list command - only package names, descriptions in preview
    let list_cmd = manager.list_available_command();

    // Preview command
    let preview_cmd = manager.show_package_command().replace("{package}", "{1}");

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages to install")
        .header("Tab to select multiple, Enter to confirm")
        .responsive_layout()
        .args([
            "--preview",
            preview_cmd.as_str(),
            "--preview-window",
            "down:40%:wrap",
            "--layout",
            "reverse-list",
            "--height",
            "90%",
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
