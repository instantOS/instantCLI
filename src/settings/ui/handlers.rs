//! Settings handlers
//!
//! Functions for handling setting interactions in the UI.

use anyhow::Result;

use crate::common::package::{Dependency, InstallResult, ensure_all};
use crate::menu_utils::FzfWrapper;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Requirement, Setting};

/// Check requirements for a setting, prompting installation if needed
fn ensure_requirements(setting: &'static dyn Setting) -> Result<bool> {
    let metadata = setting.metadata();

    // Split requirements into dependencies and other conditions
    let mut required_deps: Vec<&'static Dependency> = Vec::new();
    let mut unsatisfied_conditions = Vec::new();

    for requirement in metadata.requirements {
        match requirement {
            Requirement::Dependency(dep) => {
                if !dep.is_installed() {
                    required_deps.push(dep);
                }
            }
            Requirement::Condition { check, .. } => {
                if !check() {
                    unsatisfied_conditions.push(requirement);
                }
            }
        }
    }

    // 1. Batch install any missing dependencies
    if !required_deps.is_empty() {
        match ensure_all(&required_deps)? {
            InstallResult::Installed | InstallResult::AlreadyInstalled => {}
            InstallResult::Declined
            | InstallResult::NotAvailable { .. }
            | InstallResult::Failed { .. } => {
                return Ok(false);
            }
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
