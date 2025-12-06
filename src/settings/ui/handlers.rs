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
    let mut unmet = Vec::new();

    for requirement in metadata.requirements {
        if !requirement.is_satisfied() {
            unmet.push(requirement);
        }
    }

    if unmet.is_empty() {
        return Ok(true);
    }

    // Try to install missing packages
    for req in &unmet {
        if let Requirement::Package(pkg) = req {
            if !pkg.ensure()? {
                let mut messages = Vec::new();
                messages.push(format!(
                    "Cannot use '{}' - requirements not met:",
                    metadata.title
                ));
                messages.push(String::new());
                for r in &unmet {
                    messages.push(format!("  • {}", r.description()));
                    messages.push(format!("    {}", r.resolve_hint()));
                    messages.push(String::new());
                }

                FzfWrapper::builder()
                    .message(messages.join("\n"))
                    .title("Requirements Not Met")
                    .show_message()?;

                return Ok(false);
            }
        }
    }

    Ok(true)
}

/// Handle a setting action
pub fn handle_trait_setting(
    ctx: &mut SettingsContext,
    setting: &'static dyn Setting,
) -> Result<()> {
    // Check requirements before applying
    if !setting.metadata().requirements.is_empty() && !ensure_requirements(setting)? {
        return Ok(());
    }

    // Apply the setting
    setting.apply(ctx)?;
    ctx.persist()?;

    Ok(())
}
