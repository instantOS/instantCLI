//! Audio system checks - ensures audio server is properly configured

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::process::Command;

#[derive(Default)]
pub struct PipewireSessionManagerCheck;

impl PipewireSessionManagerCheck {
    fn is_pipewire_active() -> Result<bool> {
        // Check if pipewire daemon is running
        let output = Command::new("systemctl")
            .args(&["--user", "is-active", "pipewire.service"])
            .output()
            .context("Failed to check pipewire service status")?;

        Ok(output.status.success())
    }

    fn get_session_manager() -> Result<Option<String>> {
        // Check which session manager is running
        // First try wireplumber
        let wp_output = Command::new("systemctl")
            .args(&["--user", "is-active", "wireplumber.service"])
            .output()
            .context("Failed to check wireplumber service status")?;

        if wp_output.status.success() {
            return Ok(Some("wireplumber".to_string()));
        }

        // Then try pipewire-media-session
        let pms_output = Command::new("systemctl")
            .args(&["--user", "is-active", "pipewire-media-session.service"])
            .output()
            .context("Failed to check pipewire-media-session service status")?;

        if pms_output.status.success() {
            return Ok(Some("pipewire-media-session".to_string()));
        }

        Ok(None)
    }
}

#[async_trait]
impl DoctorCheck for PipewireSessionManagerCheck {
    fn name(&self) -> &'static str {
        "PipeWire Session Manager"
    }

    fn id(&self) -> &'static str {
        "pipewire-session-manager"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User // systemctl --user runs as user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        // Check if pipewire is even active
        match Self::is_pipewire_active() {
            Ok(true) => {
                // PipeWire is active, check session manager
                match Self::get_session_manager() {
                    Ok(Some(manager)) => {
                        if manager == "pipewire-media-session" {
                            CheckStatus::Warning {
                                message: "Using pipewire-media-session (deprecated; wireplumber recommended)"
                                    .to_string(),
                                fixable: false,
                            }
                        } else if manager == "wireplumber" {
                            CheckStatus::Pass(
                                "Using wireplumber (recommended session manager)".to_string(),
                            )
                        } else {
                            CheckStatus::Pass(format!("Using {} session manager", manager))
                        }
                    }
                    Ok(None) => CheckStatus::Fail {
                        message: "PipeWire is active but no session manager is running".to_string(),
                        fixable: false,
                    },
                    Err(e) => CheckStatus::Fail {
                        message: format!("Failed to detect session manager: {}", e),
                        fixable: false,
                    },
                }
            }
            Ok(false) => CheckStatus::Skipped("PipeWire is not active".to_string()),
            Err(e) => CheckStatus::Fail {
                message: format!("Failed to check PipeWire status: {}", e),
                fixable: false,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some(
            "Install wireplumber with: pacman -S wireplumber, then enable it with: systemctl --user enable --now wireplumber"
                .to_string(),
        )
    }
}
