//! Settings handlers
//!
//! Functions for handling setting interactions in the UI.

use anyhow::Result;

use crate::common::package::{Dependency, InstallResult, ensure_all};
use crate::settings::context::SettingsContext;
use crate::settings::setting::Setting;

/// Result of checking requirements for a setting
enum RequirementsResult {
    /// All requirements are satisfied
    Satisfied,
    /// User declined installation
    Declined,
    /// Required dependency is not available on this system
    NotAvailable { name: String, hint: String },
    /// Installation failed
    Failed { reason: String },
}

/// Check requirements for a setting, prompting installation if needed
fn ensure_requirements(setting: &'static dyn Setting) -> Result<RequirementsResult> {
    let metadata = setting.metadata();

    // Check which dependencies are missing
    let mut required_deps: Vec<&'static Dependency> = Vec::new();

    for dep in metadata.requirements {
        if !dep.is_installed() {
            required_deps.push(dep);
        }
    }

    // Batch install any missing dependencies
    if !required_deps.is_empty() {
        match ensure_all(&required_deps)? {
            InstallResult::Installed | InstallResult::AlreadyInstalled => {}
            InstallResult::Declined => {
                return Ok(RequirementsResult::Declined);
            }
            InstallResult::NotAvailable { name, hint } => {
                return Ok(RequirementsResult::NotAvailable { name, hint });
            }
            InstallResult::Failed { reason } => {
                return Ok(RequirementsResult::Failed { reason });
            }
        }
    }

    Ok(RequirementsResult::Satisfied)
}

/// Handle a setting action
pub fn handle_trait_setting(
    ctx: &mut SettingsContext,
    setting: &'static dyn Setting,
) -> Result<()> {
    // Check unsupported distros first (blacklist)
    if let Some(unsupported) = setting.metadata().unsupported_distros {
        let current_os = crate::common::distro::OperatingSystem::detect();

        if current_os.in_any_family(unsupported) {
            use crate::menu_utils::FzfWrapper;
            FzfWrapper::builder()
                .message(
                    [
                        &format!(
                            "Setting '{}' is not available on your system.",
                            setting.metadata().title
                        ),
                        &format!("Current system: {}", current_os),
                        "",
                        "This setting is not supported on:",
                        &unsupported
                            .iter()
                            .map(|d| format!("  • {}", d))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ]
                    .join("\n"),
                )
                .title("Not Supported")
                .message_dialog()?;

            return Ok(());
        }
    }

    // Check supported distros (whitelist)
    if let Some(supported) = setting.metadata().supported_distros {
        let current_os = crate::common::distro::OperatingSystem::detect();

        if !current_os.in_any_family(supported) {
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
                .message_dialog()?;

            return Ok(());
        }
    }

    // Check requirements before applying
    if !setting.metadata().requirements.is_empty() {
        match ensure_requirements(setting)? {
            RequirementsResult::Satisfied => {}
            RequirementsResult::Declined => {
                // User declined installation, just return without applying
                return Ok(());
            }
            RequirementsResult::NotAvailable { name, hint } => {
                use crate::menu_utils::FzfWrapper;
                let os = crate::common::distro::OperatingSystem::detect();
                let mut messages = Vec::new();
                messages.push(format!(
                    "'{}' requires '{}' which is not available on your system.",
                    setting.metadata().title,
                    name
                ));
                messages.push(String::new());

                if os.is_immutable() {
                    messages.push(format!("Your system ({}) is immutable and packages cannot be installed via the traditional package manager.", os));
                    messages.push(String::new());
                    messages.push(hint);
                    messages.push(String::new());
                    messages.push("Alternative installation methods:".to_string());
                    messages.push("  • Install via Flatpak if available".to_string());
                    messages.push("  • Use distrobox or another container".to_string());
                    messages.push("  • Install on the host system if supported".to_string());
                } else {
                    messages.push(hint);
                }

                FzfWrapper::builder()
                    .message(messages.join("\n"))
                    .title("Dependency Not Available")
                    .message_dialog()?;

                return Ok(());
            }
            RequirementsResult::Failed { reason } => {
                use crate::menu_utils::FzfWrapper;
                let mut messages = Vec::new();
                messages.push(format!(
                    "Failed to install required dependencies for '{}':",
                    setting.metadata().title
                ));
                messages.push(String::new());
                messages.push(reason);

                FzfWrapper::builder()
                    .message(messages.join("\n"))
                    .title("Installation Failed")
                    .message_dialog()?;

                return Ok(());
            }
        }
    }

    // Apply the setting
    setting.apply(ctx)?;
    ctx.persist()?;

    Ok(())
}
