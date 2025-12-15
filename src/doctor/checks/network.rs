use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

#[derive(Default)]
pub struct InternetCheck;

#[async_trait]
impl DoctorCheck for InternetCheck {
    fn name(&self) -> &'static str {
        "Internet Connectivity"
    }

    fn id(&self) -> &'static str {
        "internet"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User // nmtui should run as user
    }

    async fn execute(&self) -> CheckStatus {
        let has_internet = tokio::task::spawn_blocking(crate::common::network::check_internet)
            .await
            .unwrap_or(false);

        if has_internet {
            CheckStatus::Pass("Internet connection is available".to_string())
        } else {
            CheckStatus::Fail {
                message: "No internet connection detected".to_string(),
                fixable: true, // nmtui can potentially fix network issues
            }
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Run nmtui to configure your network interface.".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let status = TokioCommand::new("nmtui").status().await?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("nmtui failed to run"))
        }
    }
}

#[derive(Default)]
pub struct InstantRepoCheck;

#[async_trait]
impl DoctorCheck for InstantRepoCheck {
    fn name(&self) -> &'static str {
        "InstantOS Repository Configuration"
    }

    fn id(&self) -> &'static str {
        "instant-repo"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any // Can read config as any user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // Modifying /etc/pacman.conf requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Only check on instantOS
        if !matches!(
            crate::common::distro::OperatingSystem::detect(),
            crate::common::distro::OperatingSystem::InstantOS
        ) {
            return CheckStatus::Skipped("Not running on instantOS".to_string());
        }

        // Check if /etc/pacman.conf contains [instant] section
        match tokio::fs::read_to_string("/etc/pacman.conf").await {
            Ok(content) => {
                if content.contains("[instant]")
                    && content.contains("/etc/pacman.d/instantmirrorlist")
                {
                    CheckStatus::Pass("InstantOS repository is configured".to_string())
                } else {
                    CheckStatus::Fail {
                        message: "InstantOS repository not found in pacman.conf".to_string(),
                        fixable: true, // We can add the repository configuration
                    }
                }
            }
            Err(_) => CheckStatus::Fail {
                message: "Could not read /etc/pacman.conf".to_string(),
                fixable: false, // If we can't read the file, we probably can't fix it either
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Add InstantOS repository configuration to /etc/pacman.conf".to_string())
    }

    async fn fix(&self) -> Result<()> {
        crate::common::pacman::setup_instant_repo(false).await
    }
}
