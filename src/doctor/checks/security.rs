use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

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
        
        // A better check might be to look for known agent processes first.
        let agents = [
            "polkit-gnome-authentication-agent-1",
            "polkit-kde-authentication-agent-1",
            "lxpolkit",
            "lxqt-policykit-agent",
            "mate-polkit",
            "polkit-mate-authentication-agent-1",
            "ts-polkitagent",
            "ukui-polkit-agent",
            "pantheon-polkit-agent",
            "polkit-dumb-agent",
        ];

        let mut agent_found = false;
        for agent in agents {
             if TokioCommand::new("pgrep")
                .arg("-f")
                .arg(agent)
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                agent_found = true;
                break;
            }
        }

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
         Err(anyhow::anyhow!("Please install a polkit agent manually. Common options: polkit-gnome, lxpolkit, lxqt-policykit."))
    }
}