use crate::common::dependencies::Package;
use crate::common::requirements::ensure_packages_batch;
use crate::ui::prelude::*;
use anyhow::{Context, Result};

use super::registry::AssistAction;

/// Execute an assist action, ensuring requirements are satisfied first
pub fn execute_assist(assist: &AssistAction) -> Result<()> {
    if !assist.dependencies.is_empty() {
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

        for dependency in assist.dependencies {
            let mut checks_passed = true;
            for check in dependency.checks {
                if !check()? {
                    checks_passed = false;
                    break;
                }
            }

            if checks_passed {
                match &dependency.package {
                    Package::Os(pkg) => {
                        let all_satisfied = ensure_packages_batch(&[**pkg])
                            .context("Failed to ensure OS packages")?;
                        if !all_satisfied {
                            emit(
                                Level::Warn,
                                "assist.dependencies_not_met",
                                &format!(
                                    "{} OS package dependencies not met for {}",
                                    char::from(NerdFont::Warning),
                                    assist.description
                                ),
                                None,
                            );
                            return Ok(());
                        }
                    }
                    Package::Flatpak(fp) => {
                        let satisfied =
                            fp.ensure().context("Failed to ensure Flatpak dependency")?;
                        if !satisfied {
                            emit(
                                Level::Warn,
                                "assist.dependencies_not_met",
                                &format!(
                                    "{} Flatpak dependencies not met for {}",
                                    char::from(NerdFont::Warning),
                                    assist.description
                                ),
                                None,
                            );
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    // Execute the assist
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
