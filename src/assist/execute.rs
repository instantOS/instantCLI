use crate::assist::utils;
use crate::common::dependencies::Package;
use crate::common::requirements::ensure_packages_batch;
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

/// Install dependencies for the given assist in the current terminal context
pub fn install_dependencies_for_assist(assist: &AssistAction) -> Result<crate::common::requirements::PackageStatus> {
    if assist.dependencies.is_empty() {
        return Ok(crate::common::requirements::PackageStatus::Installed);
    }

    for dependency in assist.dependencies {
        if dependency.is_satisfied() {
            continue;
        }

        match &dependency.package {
            Package::Os(pkg) => {
                let status =
                    ensure_packages_batch(&[**pkg]).context("Failed to ensure OS packages")?;
                if !status.is_installed() {
                    return Ok(status);
                }
            }
            Package::Flatpak(fp) => {
                let status = fp.ensure().context("Failed to ensure Flatpak dependency")?;
                if !status.is_installed() {
                    return Ok(status);
                }
            }
        }

        if !dependency.is_satisfied() {
            // Should not happen if ensure succeeded, but theoretically possible
            return Ok(crate::common::requirements::PackageStatus::Failed);
        }
    }

    Ok(crate::common::requirements::PackageStatus::Installed)
}

fn ensure_dependencies_ready(assist: &AssistAction, key_sequence: &str) -> Result<bool> {
    if assist.dependencies.is_empty()
        || assist
            .dependencies
            .iter()
            .all(|dependency| dependency.is_satisfied())
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
    } else {
        if install_dependencies_via_terminal(assist, key_sequence)? {
            crate::common::requirements::PackageStatus::Installed
        } else {
            crate::common::requirements::PackageStatus::Failed
        }
    };

    if status.is_installed() {
        // Double check they are actually satisfied
        if assist.dependencies.iter().all(|d| d.is_satisfied()) {
             Ok(true)
        } else {
             emit_dependency_warning(assist);
             Ok(false)
        }
    } else if matches!(status, crate::common::requirements::PackageStatus::Declined) {
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
