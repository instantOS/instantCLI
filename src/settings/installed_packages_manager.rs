//! Installed packages manager
//!
//! Interactive package browser and uninstaller for installed system packages.

use anyhow::{Context, Result};

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, uninstall_packages};
use crate::menu_utils::{ConfirmResult, DecodedStreamingMenuItem, FzfResult, FzfWrapper, Header};
use crate::settings::package_list::{self, PackageSelectionPayload};
use crate::ui::catppuccin::fzf_mocha_args;

enum UninstallResult {
    Done,
    Uninstalled,
    Cancelled,
}

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

/// Run the Snap app manager.
pub fn run_snap_uninstaller(debug: bool) -> Result<()> {
    run_uninstaller(PackageManager::Snap, debug)
}

/// Run the package uninstaller for any supported package manager.
fn run_uninstaller(manager: PackageManager, debug: bool) -> Result<()> {
    if debug {
        println!("Starting {} package manager...", manager);
    }

    if !manager.is_available() {
        anyhow::bail!("{} is not available on this system", manager);
    }

    loop {
        let result = FzfWrapper::builder()
            .multi_select(true)
            .prompt("Select packages")
            .header(Header::fancy(&format!(
                "Manage Installed {}",
                manager.display_name()
            )))
            .args(fzf_mocha_args())
            .responsive_layout()
            .select_encoded_streaming(package_list::installed_command(manager))
            .context("Failed to run package selector")?;

        match handle_uninstall_result(result, manager, debug)? {
            UninstallResult::Done => break,
            UninstallResult::Uninstalled => {}
            UninstallResult::Cancelled => break,
        }
    }

    Ok(())
}

/// Handle the uninstall result.
fn handle_uninstall_result(
    result: FzfResult<DecodedStreamingMenuItem<PackageSelectionPayload>>,
    manager: PackageManager,
    debug: bool,
) -> Result<UninstallResult> {
    match result {
        FzfResult::MultiSelected(rows) if !rows.is_empty() => {
            let packages: Vec<String> = rows
                .into_iter()
                .map(|row| row.payload.package)
                .filter(|s| !s.is_empty())
                .collect();

            if packages.is_empty() {
                println!("No valid packages selected.");
                return Ok(UninstallResult::Done);
            }

            if debug {
                println!("Selected packages: {:?}", packages);
            }

            let msg = format!(
                "Uninstall {} {} package{}?\n\nThis action cannot be undone.",
                packages.len(),
                manager.display_name(),
                if packages.len() == 1 { "" } else { "s" }
            );

            if !confirm_uninstall(&msg)? {
                println!("Uninstall cancelled.");
                return Ok(UninstallResult::Done);
            }

            let refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            uninstall_packages(manager, &refs)?;

            println!("✓ Package uninstallation completed successfully!");
            Ok(UninstallResult::Uninstalled)
        }
        FzfResult::Selected(row) => {
            let name = row.payload.package;

            if debug {
                println!("Selected package: {}", name);
            }

            let msg = format!(
                "Uninstall {} package '{}'?\n\nThis action cannot be undone.",
                manager.display_name(),
                name
            );

            if !confirm_uninstall(&msg)? {
                println!("Uninstall cancelled.");
                return Ok(UninstallResult::Done);
            }

            uninstall_packages(manager, &[&name])?;

            println!("✓ Package uninstallation completed successfully!");
            Ok(UninstallResult::Uninstalled)
        }
        FzfResult::MultiSelected(_) | FzfResult::Cancelled => Ok(UninstallResult::Cancelled),
        FzfResult::Error(err) => anyhow::bail!("Package selection failed: {}", err),
    }
}

/// Show uninstall confirmation dialog.
fn confirm_uninstall(message: &str) -> Result<bool> {
    let result = FzfWrapper::builder()
        .confirm(message)
        .yes_text("Uninstall")
        .no_text("Cancel")
        .confirm_dialog()?;
    Ok(matches!(result, ConfirmResult::Yes))
}
