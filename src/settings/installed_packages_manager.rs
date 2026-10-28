//! Installed packages manager
//!
//! Interactive package browser and uninstaller for installed system packages.

use std::collections::BTreeSet;

use anyhow::{Context, Result};

use crate::common::distro::OperatingSystem;
use crate::common::package::{
    PackageDatabaseLocked, PackageManager, removal_cascade, uninstall_packages,
};
use crate::menu_utils::{
    ConfirmResult, DecodedStreamingMenuItem, FzfWrapper, HeaderBuilder, MenuSelection,
};
use crate::settings::package_list::{self, PackageSelectionPayload};
use crate::ui::nerd_font::NerdFont;

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
            .prompt("Select packages")
            .header(
                HeaderBuilder::new(NerdFont::Package, "Manage Installed Packages")
                    .field("Package manager", manager.display_name())
                    .build(),
            )
            .responsive_layout()
            .command(package_list::installed_command(manager))
            .select_many()
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
    result: crate::menu_utils::DialogOutcome<
        MenuSelection<DecodedStreamingMenuItem<PackageSelectionPayload>>,
    >,
    manager: PackageManager,
    debug: bool,
) -> Result<UninstallResult> {
    match result {
        crate::menu_utils::DialogOutcome::Submitted(sel) if !sel.items.is_empty() => {
            let packages = deduplicate_packages(
                sel.items
                    .into_iter()
                    .map(|row| row.payload.package)
                    .filter(|s| !s.is_empty()),
            );

            if packages.is_empty() {
                println!("No valid packages selected.");
                return Ok(UninstallResult::Done);
            }

            if debug {
                println!("Selected packages: {:?}", packages);
            }

            let Some(packages) = prepare_uninstall_packages(manager, packages, debug)? else {
                println!("Uninstall cancelled.");
                return Ok(UninstallResult::Done);
            };

            let msg = uninstall_confirmation_message(manager, &packages);

            if !confirm_uninstall(&msg)? {
                println!("Uninstall cancelled.");
                return Ok(UninstallResult::Done);
            }

            let refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
            if !retry_package_operation(manager, || uninstall_packages(manager, &refs))? {
                return Ok(UninstallResult::Cancelled);
            }

            println!(
                "{} Package uninstallation completed successfully!",
                NerdFont::Check
            );
            Ok(UninstallResult::Uninstalled)
        }
        crate::menu_utils::DialogOutcome::Submitted(_)
        | crate::menu_utils::DialogOutcome::Cancelled => Ok(UninstallResult::Cancelled),
    }
}

/// Keep the confirmed selection alive while the user resolves a failure.
pub(super) fn retry_package_operation(
    manager: PackageManager,
    mut operation: impl FnMut() -> Result<()>,
) -> Result<bool> {
    loop {
        match operation() {
            Ok(()) => return Ok(true),
            Err(error) => {
                let guidance = if error.downcast_ref::<PackageDatabaseLocked>().is_some() {
                    "Another package manager may be running. Let it finish, then retry."
                } else {
                    "Resolve the error below, then retry. If another package manager is running, let it finish first. Some packages may already have been removed."
                };
                let message = format!(
                    "{manager} could not complete the operation.\n\n{guidance}\n\nYour package selection is kept for retry.\n\n{error:#}"
                );
                if !matches!(
                    FzfWrapper::builder()
                        .confirm(message)
                        .yes_text("Retry")
                        .no_text("Quit")
                        .confirm_dialog()?,
                    ConfirmResult::Yes
                ) {
                    return Ok(false);
                }
            }
        }
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

fn prepare_uninstall_packages(
    manager: PackageManager,
    packages: Vec<String>,
    debug: bool,
) -> Result<Option<Vec<String>>> {
    let dependents = removal_cascade(manager, &packages, None)?;
    if dependents.is_empty() {
        return Ok(Some(packages));
    }

    if debug {
        println!(
            "Dependent packages that also need removal: {:?}",
            dependents
        );
    }

    let dependent_list = format_package_list(&dependents);
    let msg = format!(
        "Some installed packages depend on your selection:\n\n{}\n\nAdd {} dependent package{} to the uninstall list?",
        dependent_list,
        dependents.len(),
        if dependents.len() == 1 { "" } else { "s" }
    );

    if !confirm_add_dependents(&msg)? {
        return Ok(None);
    }

    let mut expanded = packages;
    expanded.extend(dependents);
    Ok(Some(deduplicate_packages(expanded)))
}

fn confirm_add_dependents(message: &str) -> Result<bool> {
    let result = FzfWrapper::builder()
        .confirm(message)
        .yes_text("Add dependents")
        .no_text("Cancel")
        .confirm_dialog()?;
    Ok(matches!(result, ConfirmResult::Yes))
}

fn deduplicate_packages(packages: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    packages
        .into_iter()
        .filter(|package| seen.insert(package.clone()))
        .collect()
}

fn format_package_list(packages: &[String]) -> String {
    const MAX_SHOWN: usize = 12;

    let mut lines: Vec<String> = packages
        .iter()
        .take(MAX_SHOWN)
        .map(|package| format!("  - {package}"))
        .collect();

    if packages.len() > MAX_SHOWN {
        lines.push(format!("  - ... and {} more", packages.len() - MAX_SHOWN));
    }

    lines.join("\n")
}

fn uninstall_confirmation_message(manager: PackageManager, packages: &[String]) -> String {
    format!(
        "Uninstall {} {} package{}?\n\n{}\n\nThis action cannot be undone.",
        packages.len(),
        manager.display_name(),
        if packages.len() == 1 { "" } else { "s" },
        format_package_list(packages)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_failed_operation_without_reselecting_packages() -> Result<()> {
        let _guard = crate::menu_utils::MockQueue::new().confirm_yes().guard();
        let mut attempts = 0;
        assert!(retry_package_operation(PackageManager::Apt, || {
            attempts += 1;
            if attempts == 1 {
                anyhow::bail!("database busy");
            }
            Ok(())
        })?);
        assert_eq!(attempts, 2);
        Ok(())
    }

    #[test]
    fn dismissing_recovery_does_not_retry() -> Result<()> {
        let _guard = crate::menu_utils::MockQueue::new()
            .confirm_cancelled()
            .guard();
        assert!(!retry_package_operation(PackageManager::Apt, || {
            anyhow::bail!("database busy")
        })?);
        Ok(())
    }

    #[test]
    fn quitting_after_lock_does_not_retry() -> Result<()> {
        let _guard = crate::menu_utils::MockQueue::new().confirm_no().guard();
        let mut attempts = 0;
        assert!(!retry_package_operation(PackageManager::Pacman, || {
            attempts += 1;
            Err(PackageDatabaseLocked {
                manager: PackageManager::Pacman,
                detail: "database locked".to_string(),
            }
            .into())
        })?);
        assert_eq!(attempts, 1);
        Ok(())
    }

    #[test]
    fn deduplicates_packages_without_reordering_first_occurrences() {
        let packages = vec![
            "one".to_string(),
            "two".to_string(),
            "one".to_string(),
            "three".to_string(),
        ];

        assert_eq!(
            deduplicate_packages(packages),
            vec!["one".to_string(), "two".to_string(), "three".to_string()]
        );
    }
}
