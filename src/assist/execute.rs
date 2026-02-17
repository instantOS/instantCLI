use crate::assist::utils;
use crate::common::package::{InstallResult, ensure_all};
use crate::common::shell::shell_quote;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use std::io::IsTerminal;

use super::registry::AssistAction;

/// Execute an assist action, ensuring requirements are satisfied first.
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

/// Install dependencies for the given assist using batched installation.
pub fn install_dependencies_for_assist(assist: &AssistAction) -> Result<InstallResult> {
    if assist.dependencies.is_empty() {
        return Ok(InstallResult::AlreadyInstalled);
    }
    ensure_all(assist.dependencies)
}

fn ensure_dependencies_ready(assist: &AssistAction, key_sequence: &str) -> Result<bool> {
    if assist.dependencies.is_empty() || assist.dependencies.iter().all(|d| d.is_installed()) {
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

    let result = if std::io::stdout().is_terminal() {
        install_dependencies_for_assist(assist)?
    } else if install_dependencies_via_terminal(assist, key_sequence)? {
        InstallResult::Installed
    } else {
        InstallResult::Failed {
            reason: "Terminal installation failed".to_string(),
        }
    };

    match result {
        InstallResult::AlreadyInstalled | InstallResult::Installed => {
            if assist.dependencies.iter().all(|d| d.is_installed()) {
                Ok(true)
            } else {
                emit_dependency_warning(assist);
                Ok(false)
            }
        }
        InstallResult::Declined => {
            emit(
                Level::Info,
                "assist.cancelled",
                "Assist execution cancelled.",
                None,
            );
            Ok(false)
        }
        InstallResult::NotAvailable { name, hint } => {
            emit(
                Level::Warn,
                "assist.not_available",
                &format!(
                    "{} {} is not available: {}",
                    char::from(NerdFont::Warning),
                    name,
                    hint
                ),
                None,
            );
            Ok(false)
        }
        InstallResult::Failed { reason } => {
            emit(
                Level::Error,
                "assist.install_failed",
                &format!("{} {}", char::from(NerdFont::CrossCircle), reason),
                None,
            );
            Ok(false)
        }
    }
}

fn install_dependencies_via_terminal(assist: &AssistAction, key_sequence: &str) -> Result<bool> {
    let binary = std::env::current_exe()?;
    let command = format!(
        "{} assist install-deps --key-sequence {}",
        shell_quote(&binary.to_string_lossy()),
        shell_quote(key_sequence)
    );
    let title = format!("Install dependencies for {}", assist.description);
    Ok(utils::run_installation_in_terminal(&command, &title)?.success())
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
