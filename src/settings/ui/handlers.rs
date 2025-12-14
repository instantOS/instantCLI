//! Settings handlers
//!
//! Functions for handling setting interactions in the UI.

use anyhow::Result;

use crate::menu_utils::FzfWrapper;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Requirement, Setting};

/// Check requirements for a setting, prompting installation if needed
fn ensure_requirements(setting: &'static dyn Setting) -> Result<bool> {
    let metadata = setting.metadata();

    // Split requirements into packages and other conditions
    let mut required_packages = Vec::new();
    let mut unsatisfied_conditions = Vec::new();

    for requirement in metadata.requirements {
        match requirement {
            Requirement::Package(pkg) => {
                if !pkg.is_installed() {
                    required_packages.push(*pkg);
                }
            }
            Requirement::Condition { check, .. } => {
                if !check() {
                    unsatisfied_conditions.push(requirement);
                }
            }
        }
    }

    // 1. Batch install any missing packages
    if !required_packages.is_empty() {
        // This handles prompting, installing, and reporting errors for packages
        let status = crate::common::requirements::ensure_packages_batch(&required_packages)?;
        if !status.is_installed() {
            // If declined or failed, we return false.
            // Note: ensure_packages_batch already showed success/fail/decline messages.
            return Ok(false);
        }
    }

    // 2. Check remaining custom conditions
    if unsatisfied_conditions.is_empty() {
        return Ok(true);
    }

    // If we have unsatisfied custom conditions, show them
    let mut messages = Vec::new();
    messages.push(format!(
        "Cannot use '{}' - requirements not met:",
        metadata.title
    ));
    messages.push(String::new());

    for req in &unsatisfied_conditions {
        messages.push(format!("  • {}", req.description()));
        messages.push(format!("    {}", req.resolve_hint()));
        messages.push(String::new());
    }

    FzfWrapper::builder()
        .message(messages.join("\n"))
        .title("Requirements Not Met")
        .show_message()?;

    Ok(false)
}

/// Handle a setting action
pub fn handle_trait_setting(
    ctx: &mut SettingsContext,
    setting: &'static dyn Setting,
) -> Result<()> {
    // Check supported distros
    if let Some(supported) = setting.metadata().supported_distros {
        let current_os = crate::common::distro::OperatingSystem::detect();

        if !current_os.is_supported_by(supported) {
            use crate::menu_utils::FzfWrapper;
            let mut messages = Vec::new();
            messages.push(format!(
                "Setting '{}' is not available on your system.",
                setting.metadata().title
            ));
            messages.push(format!("Current system: {}", current_os));
            messages.push(String::new());
            messages.push("Supported systems:".to_string());
            for distro in supported {
                messages.push(format!("  • {}", distro));
            }

            FzfWrapper::builder()
                .message(messages.join("\n"))
                .title("Not Supported")
                .show_message()?;

            return Ok(());
        }
    }

    // Check requirements before applying
    if !setting.metadata().requirements.is_empty() && !ensure_requirements(setting)? {
        return Ok(());
    }

    // Apply the setting
    setting.apply(ctx)?;
    ctx.persist()?;

    Ok(())
}
