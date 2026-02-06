//! Installed packages manager
//!
//! Interactive package browser and uninstaller for installed system packages.

use anyhow::{Context, Result};

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, uninstall_packages};
use crate::menu_utils::{ConfirmResult, FzfResult, FzfWrapper, Header};
use crate::preview::{PreviewId, preview_command_streaming};
use crate::ui::catppuccin::fzf_mocha_args;

/// Run the installed packages manager.
/// Dispatches to the appropriate package manager based on the detected OS.
pub fn run_installed_packages_manager(debug: bool) -> Result<()> {
    let os = OperatingSystem::detect();

    if let Some(manager) = os.native_package_manager() {
        run_uninstaller(manager, debug)
    } else {
        anyhow::bail!(
            "Package manager not supported on this system ({})",
            os.name()
        )
    }
}

/// Run the package uninstaller for any supported package manager.
fn run_uninstaller(manager: PackageManager, debug: bool) -> Result<()> {
    if debug {
        println!("Starting {} package manager...", manager);
    }

    if !manager.is_available() {
        anyhow::bail!("{} is not available on this system", manager);
    }

    let list_cmd = manager.list_installed_command();
    let preview_cmd = preview_command_streaming(PreviewId::InstalledPackage);

    let result = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select packages")
        .header(Header::fancy("Manage Installed Packages"))
        .args(fzf_mocha_args())
        .args(["--preview", &preview_cmd, "--ansi"])
        .responsive_layout()
        .select_streaming(list_cmd)
        .context("Failed to run package selector")?;

    handle_uninstall_result(result, manager, debug)
}

/// Handle the uninstall result.
fn handle_uninstall_result(
    result: FzfResult<String>,
    manager: PackageManager,
    debug: bool,
) -> Result<()> {
    match result {
        FzfResult::MultiSelected(lines) if !lines.is_empty() => {
            let packages: Vec<String> = lines.into_iter()
                .map(|l| l.trim().to_string())
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
                "Uninstall {} package{}?\n\nThis action cannot be undone.",
                packages.len(),
                if packages.len() == 1 { "" } else { "s" }
            );

            if !confirm_uninstall(&msg)? {
                println!("Uninstall cancelled.");
                return Ok(());
            }

            let refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            uninstall_packages(manager, &refs)?;

            println!("✓ Package uninstallation completed successfully!");
            Ok(())
        }
        FzfResult::Selected(line) => {
            let name = line.trim();

            if debug {
                println!("Selected package: {}", name);
            }

            let msg = format!(
                "Uninstall package '{}'?\n\nThis action cannot be undone.",
                name
            );

            if !confirm_uninstall(&msg)? {
                println!("Uninstall cancelled.");
                return Ok(());
            }

            uninstall_packages(manager, &[name])?;

            println!("✓ Package uninstallation completed successfully!");
            Ok(())
        }
        FzfResult::MultiSelected(_) | FzfResult::Cancelled => {
            println!("No packages selected.");
            Ok(())
        }
        FzfResult::Error(err) => anyhow::bail!("Package selection failed: {}", err),
    }
}

/// Show uninstall confirmation dialog.
fn confirm_uninstall(message: &str) -> Result<bool> {
    let result = FzfWrapper::builder()
        .confirm(message)
        .yes_text("Uninstall")
        .no_text("Cancel")
        .show_confirmation()?;
    Ok(matches!(result, ConfirmResult::Yes))
}
