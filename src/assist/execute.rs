use crate::assist::utils;
use crate::common::package::Dependency;
use crate::common::requirements::PackageStatus;
use crate::common::shell::shell_quote;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use std::io::IsTerminal;

use super::registry::AssistAction;

/// Execute an assist action, ensuring requirements are satisfied first
pub fn execute_assist(assist: &AssistAction, key_sequence: &str) -> Result<()> {
    if !ensure_dependencies_ready(assist, key_sequence)? {
        return Ok(());
    }

    emit(
        Level::Info,
        "assist.executing",
        &format!(
            "{} Executing {}...",
            char::from(assist.icon),
            assist.description
        ),
        None,
    );

    (assist.execute)().context(format!("Failed to execute assist: {}", assist.description))?;

    emit(
        Level::Success,
        "assist.executed",
        &format!(
            "{} {} launched successfully",
            char::from(NerdFont::Check),
            assist.description
        ),
        None,
    );

    Ok(())
}

/// Install dependencies for the given assist using the new unified package system.
///
/// This uses the new `Dependency` type from `common::package` which supports
/// multiple package managers with automatic fallback.
pub fn install_dependencies_for_assist(assist: &AssistAction) -> Result<PackageStatus> {
    if assist.dependencies.is_empty() {
        return Ok(PackageStatus::Installed);
    }

    // Collect all dependencies that need to be installed
    let missing: Vec<&&Dependency> = assist
        .dependencies
        .iter()
        .filter(|dep| !dep.is_installed())
        .collect();

    if missing.is_empty() {
        return Ok(PackageStatus::Installed);
    }

    // Install each dependency individually using the best available package manager
    for dep in &missing {
        // We need to handle static lifetime requirement
        // For now, we'll install each dependency individually
        if let Some(pkg_def) = dep.get_best_package() {
            // Check if the package manager is available
            if !pkg_def.manager.is_available() {
                emit(
                    Level::Warn,
                    "assist.package_unavailable",
                    &format!(
                        "No supported package manager available for {}",
                        dep.name
                    ),
                    None,
                );
                continue;
            }

            // Install using the package manager
            install_single_dependency(dep)?;
        } else {
            emit(
                Level::Warn,
                "assist.no_package",
                &format!(
                    "No installable package found for {}. {}",
                    dep.name,
                    dep.install_hint()
                ),
                None,
            );
        }
    }

    // Verify all are now installed
    if assist.dependencies.iter().all(|d| d.is_installed()) {
        Ok(PackageStatus::Installed)
    } else {
        Ok(PackageStatus::Failed)
    }
}

/// Install a single dependency using the best available package manager.
fn install_single_dependency(dep: &Dependency) -> Result<()> {
    let pkg_def = match dep.get_best_package() {
        Some(p) => p,
        None => return Ok(()), // No package available
    };

    // Build confirmation message
    let msg = format!(
        "Install {} via {}?\n\n{}",
        dep.name,
        pkg_def.manager.display_name(),
        pkg_def.install_hint()
    );

    // Prompt for confirmation
    let should_install = crate::menu_utils::FzfWrapper::builder()
        .confirm(&msg)
        .yes_text("Install")
        .no_text("Cancel")
        .show_confirmation()?;

    if !matches!(should_install, crate::menu_utils::ConfirmResult::Yes) {
        return Ok(());
    }

    // Execute installation
    let (cmd, base_args) = pkg_def.manager.install_command();
    let mut args: Vec<&str> = base_args.to_vec();
    args.push(pkg_def.package_name);

    emit(
        Level::Info,
        "assist.installing",
        &format!("Installing {} via {}...", dep.name, pkg_def.manager),
        None,
    );

    // Handle AUR helper detection
    let actual_cmd = if pkg_def.manager == crate::common::package::PackageManager::Aur {
        crate::common::package::detect_aur_helper().unwrap_or("yay")
    } else {
        cmd
    };

    duct::cmd(actual_cmd, &args)
        .run()
        .with_context(|| format!("Failed to install {} via {}", dep.name, pkg_def.manager))?;

    Ok(())
}

fn ensure_dependencies_ready(assist: &AssistAction, key_sequence: &str) -> Result<bool> {
    if assist.dependencies.is_empty()
        || assist
            .dependencies
            .iter()
            .all(|dependency| dependency.is_installed())
    {
        return Ok(true);
    }

    emit(
        Level::Info,
        "assist.checking_dependencies",
        &format!(
            "{} Checking dependencies for {}...",
            char::from(NerdFont::Package),
            assist.description
        ),
        None,
    );

    let status = if std::io::stdout().is_terminal() {
        install_dependencies_for_assist(assist)?
    } else if install_dependencies_via_terminal(assist, key_sequence)? {
        PackageStatus::Installed
    } else {
        PackageStatus::Failed
    };

    if status.is_installed() {
        // Double check they are actually satisfied
        if assist.dependencies.iter().all(|d| d.is_installed()) {
            Ok(true)
        } else {
            emit_dependency_warning(assist);
            Ok(false)
        }
    } else if matches!(status, PackageStatus::Declined) {
        emit(
            Level::Info,
            "assist.cancelled",
            "Assist execution cancelled.",
            None,
        );
        Ok(false)
    } else {
        emit_dependency_warning(assist);
        Ok(false)
    }
}

fn install_dependencies_via_terminal(assist: &AssistAction, key_sequence: &str) -> Result<bool> {
    let binary = utils::current_exe()?;
    let binary_str = binary.to_string_lossy();
    let command = format!(
        "{} assist install-deps --key-sequence {}",
        shell_quote(&binary_str),
        shell_quote(key_sequence)
    );

    let script = format!("#!/usr/bin/env bash\nset -e\n{}\n", command);

    let title = format!("Install dependencies for {}", assist.description);
    let status = utils::run_script_in_terminal_and_wait(&script, &title)?;

    Ok(status.success())
}

fn emit_dependency_warning(assist: &AssistAction) {
    emit(
        Level::Warn,
        "assist.dependencies_not_met",
        &format!(
            "{} Dependencies not met for {}",
            char::from(NerdFont::Warning),
            assist.description
        ),
        None,
    );
}
