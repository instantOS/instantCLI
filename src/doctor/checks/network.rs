use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use crate::common::distro::OperatingSystem;
use crate::common::pacman_mirrors;
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command as TokioCommand;

const PACMAN_MIRRORLIST_PATH: &str = "/etc/pacman.d/mirrorlist";

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
pub struct PacmanMirrorCheck;

#[async_trait]
impl DoctorCheck for PacmanMirrorCheck {
    fn name(&self) -> &'static str {
        "Pacman Primary Mirror"
    }

    fn id(&self) -> &'static str {
        "pacman-mirror"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root
    }

    async fn execute(&self) -> CheckStatus {
        let os = OperatingSystem::detect();
        if !os.in_family(&OperatingSystem::Arch) {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
        }
        if os.is_immutable() {
            return CheckStatus::Skipped(
                "Immutable OS manages package mirrors outside pacman".to_string(),
            );
        }

        let content = match tokio::fs::read_to_string(PACMAN_MIRRORLIST_PATH).await {
            Ok(content) => content,
            Err(error) => {
                return CheckStatus::Fail {
                    message: format!("Could not read {PACMAN_MIRRORLIST_PATH}: {error}"),
                    fixable: false,
                };
            }
        };
        let mirrors = pacman_mirrors::active_mirrors(&content);
        let Some(primary) = mirrors.first() else {
            return CheckStatus::Fail {
                message: "Pacman mirrorlist has no active Server entries".to_string(),
                fixable: false,
            };
        };
        let client = match pacman_mirrors::http_client() {
            Ok(client) => client,
            Err(error) => {
                return CheckStatus::Fail {
                    message: format!("Could not initialize mirror check: {error:#}"),
                    fixable: false,
                };
            }
        };

        match pacman_mirrors::probe_mirror(&client, primary).await {
            Ok(probe) => CheckStatus::Pass(format!(
                "Primary mirror is healthy ({:.0} ms): {}",
                probe.latency.as_secs_f64() * 1000.0,
                primary.template
            )),
            Err(error) => CheckStatus::Warning {
                message: format!(
                    "Primary mirror is unhealthy: {} ({error:#})",
                    primary.template
                ),
                fixable: mirrors.len() > 1,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Test configured fallback mirrors and promote the first healthy one".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let path = Path::new(PACMAN_MIRRORLIST_PATH);
        let content = tokio::fs::read_to_string(path).await?;
        let mirrors = pacman_mirrors::active_mirrors(&content);
        let client = pacman_mirrors::http_client()?;
        let (selected, attempts) = pacman_mirrors::first_healthy_mirror(
            &client,
            &mirrors,
            pacman_mirrors::DEFAULT_PROBE_LIMIT,
        )
        .await?;
        let updated = pacman_mirrors::promote_mirror(&content, selected.mirror.line_index)?;

        if updated != content {
            pacman_mirrors::write_mirrorlist(path, &updated)?;
            println!(
                "Promoted healthy mirror after {} check(s): {}",
                attempts, selected.mirror.template
            );
        } else {
            println!("Primary mirror recovered while applying the fix; no changes needed.");
        }
        Ok(())
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
