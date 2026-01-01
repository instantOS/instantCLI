use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

/// Check if any polkit authentication agent is running
/// This detects any agent that properly registers with polkit via D-Bus
pub async fn is_polkit_agent_running() -> bool {
    // On GNOME, the polkit agent is integrated into gnome-shell
    // Use existing CompositorType detection for consistency
    use crate::common::compositor::CompositorType;
    if CompositorType::detect() == CompositorType::Gnome {
        if is_process_running("gnome-shell").await {
            return true;
        }
        if is_process_running("xdg-desktop-portal").await {
            return true;
        }
    }

    // Check for registered polkit authentication agents via D-Bus
    // This detects any agent that properly registers with polkit, regardless of process name
    TokioCommand::new("busctl")
        .arg("--user")
        .arg("list")
        .output()
        .await
        .map(|output| {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Look for any RUNNING service (has a PID) that contains "polkit" or "PolicyKit" in the name
                // Skip activatable services (those with "-" under PID column)
                stdout.lines().any(|line| {
                    let line_lower = line.to_lowercase();
                    // Must have a PID (not activatable) AND contain polkit/PolicyKit
                    (line_lower.contains("polkit") || line_lower.contains("policykit"))
                        && line.split_whitespace().nth(1).is_some_and(|pid| pid != "-")
                })
            } else {
                false
            }
        })
        .unwrap_or(false)
}

/// Check if a process with the given name is running
async fn is_process_running(process_name: &str) -> bool {
    TokioCommand::new("pgrep")
        .arg("-x")
        .arg(process_name)
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[derive(Default)]
pub struct PolkitAgentCheck;

#[async_trait]
impl DoctorCheck for PolkitAgentCheck {
    fn name(&self) -> &'static str {
        "Polkit Authentication Agent"
    }

    fn id(&self) -> &'static str {
        "polkit-agent"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        use crate::common::display_server::DisplayServer;
        use crate::common::distro::OperatingSystem;

        // Skip on Termux
        if OperatingSystem::detect() == OperatingSystem::Termux {
            return CheckStatus::Skipped("Not applicable on Termux".to_string());
        }

        // Skip if not a desktop session
        if !DisplayServer::detect().is_desktop_session() {
            return CheckStatus::Skipped("Not running in a desktop session".to_string());
        }

        // Check if polkitd is running
        let polkitd_running = TokioCommand::new("pgrep")
            .arg("polkitd")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !polkitd_running {
            return CheckStatus::Fail {
                message: "Polkit daemon (polkitd) is not running".to_string(),
                fixable: false, // System level issue
            };
        }

        // Functional check using pkcheck
        // This attempts to check authorization for rebooting, allowing user interaction.
        // If an agent is running, it should either return success (if authorized) or
        // prompt for authentication (which we can't do here, but the existence of the
        // prompt mechanism implies an agent).
        // However, pkcheck returns 0 if authorized, 1 if not authorized, 2 if not authorized
        // and no agent is available (or other errors), 3 if not authorized and dismissed.
        // We really want to know if it *fails* to find an agent.

        // Check for registered polkit authentication agents via D-Bus
        let agent_found = is_polkit_agent_running().await;

        if agent_found {
            return CheckStatus::Pass("Polkit authentication agent detected".to_string());
        }

        // If no known agent process found, try the functional test as a fallback
        // If pkcheck returns 2 or 3, it usually means no agent or cancelled.
        // But running pkcheck might pop up a dialog which is annoying for a background check.
        // So relying on process detection is cleaner for "doctor" style checks.

        CheckStatus::Fail {
            message: "No running Polkit authentication agent found".to_string(),
            fixable: false, // We can't automatically install/configure the right one for every DE
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Install a polkit agent (e.g., polkit-gnome) and add it to your autostart configuration.".to_string())
    }

    async fn fix(&self) -> Result<()> {
        Err(anyhow::anyhow!(
            "Please install a polkit agent manually. Common options: polkit-gnome, lxpolkit, lxqt-policykit."
        ))
    }
}
