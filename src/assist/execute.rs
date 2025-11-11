use crate::common::requirements::ensure_packages_batch;
use crate::ui::prelude::*;
use anyhow::{Context, Result};

use super::registry::AssistAction;

/// Execute an assist action, ensuring requirements are satisfied first
pub fn execute_assist(assist: &AssistAction) -> Result<()> {
    // Check and install requirements if needed
    if !assist.requirements.is_empty() {
        emit(
            Level::Info,
            "assist.checking_requirements",
            &format!(
                "{} Checking requirements for {}...",
                char::from(NerdFont::Package),
                assist.title
            ),
            None,
        );

        let all_satisfied =
            ensure_packages_batch(assist.requirements).context("Failed to ensure requirements")?;

        if !all_satisfied {
            emit(
                Level::Warn,
                "assist.requirements_not_met",
                &format!(
                    "{} Requirements not satisfied for {}",
                    char::from(NerdFont::Warning),
                    assist.title
                ),
                None,
            );
            return Ok(()); // Don't execute if requirements aren't met
        }
    }

    // Execute the assist
    emit(
        Level::Info,
        "assist.executing",
        &format!("{} Executing {}...", char::from(assist.icon), assist.title),
        None,
    );

    (assist.execute)().context(format!("Failed to execute assist: {}", assist.title))?;

    emit(
        Level::Success,
        "assist.executed",
        &format!(
            "{} {} launched successfully",
            char::from(NerdFont::Check),
            assist.title
        ),
        None,
    );

    Ok(())
}
