use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

#[derive(Default)]
pub struct SwapCheck;

#[async_trait]
impl DoctorCheck for SwapCheck {
    fn name(&self) -> &'static str {
        "Swap Space Availability"
    }

    fn id(&self) -> &'static str {
        "swap"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    async fn execute(&self) -> CheckStatus {
        // Read /proc/meminfo to check swap
        match tokio::fs::read_to_string("/proc/meminfo").await {
            Ok(content) => {
                for line in content.lines() {
                    if line.starts_with("SwapTotal:") {
                        // Format: SwapTotal:       16777212 kB
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2
                            && let Ok(swap_kb) = parts[1].parse::<u64>()
                        {
                            if swap_kb == 0 {
                                return CheckStatus::Warning {
                                    message: "No swap space available".to_string(),
                                    fixable: false,
                                };
                            } else {
                                let swap_gb = swap_kb as f64 / (1024.0 * 1024.0);
                                return CheckStatus::Pass(format!(
                                    "Swap space available: {:.2} GB",
                                    swap_gb
                                ));
                            }
                        }
                    }
                }
                CheckStatus::Warning {
                    message: "Could not determine swap status".to_string(),
                    fixable: false,
                }
            }
            Err(e) => CheckStatus::Fail {
                message: format!("Could not read /proc/meminfo: {}", e),
                fixable: false,
            },
        }
    }
}

#[derive(Default)]
pub struct PendingUpdatesCheck;

impl PendingUpdatesCheck {
    const WARN_THRESHOLD: usize = 50;
}

#[async_trait]
impl DoctorCheck for PendingUpdatesCheck {
    fn name(&self) -> &'static str {
        "Pending System Updates"
    }

    fn id(&self) -> &'static str {
        "pending-updates"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any // checkupdates runs as user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // pacman -Syu requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Only run on Arch-based systems
        if !crate::common::distro::OperatingSystem::detect().is_arch_based() {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
        }

        // Run checkupdates to get list of pending updates
        let output = TokioCommand::new("checkupdates").output().await;

        match output {
            Ok(output) => {
                // checkupdates exit codes (per man page):
                // 0 = updates available (outputs list)
                // 1 = unknown cause of failure
                // 2 = no updates available
                if output.status.code() == Some(2) {
                    // No updates available
                    return CheckStatus::Pass("System is up to date".to_string());
                }

                if output.status.code() == Some(1) {
                    // Unknown failure - could be temp db issue, network, stale lock, etc.
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let message = if stderr.trim().is_empty() {
                        "checkupdates failed (unknown cause - may be temp db or network issue)"
                            .to_string()
                    } else {
                        format!("checkupdates failed: {}", stderr.trim())
                    };
                    return CheckStatus::Warning {
                        message,
                        fixable: false,
                    };
                }

                if !output.status.success() {
                    return CheckStatus::Fail {
                        message: format!(
                            "checkupdates failed with exit code {:?}",
                            output.status.code()
                        ),
                        fixable: false,
                    };
                }

                // Count the number of pending updates (one per line)
                let update_count = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|line| !line.is_empty())
                    .count();

                if update_count == 0 {
                    CheckStatus::Pass("System is up to date".to_string())
                } else if update_count > Self::WARN_THRESHOLD {
                    CheckStatus::Warning {
                        message: format!(
                            "{} pending updates (exceeds {} threshold)",
                            update_count,
                            Self::WARN_THRESHOLD
                        ),
                        fixable: true,
                    }
                } else {
                    CheckStatus::Pass(format!("{} pending updates", update_count))
                }
            }
            Err(e) => {
                // Check if the error is because checkupdates is not found
                let error_msg = e.to_string();
                if error_msg.contains("No such file") || error_msg.contains("not found") {
                    CheckStatus::Fail {
                        message: "checkupdates not found (install pacman-contrib)".to_string(),
                        fixable: true,
                    }
                } else {
                    CheckStatus::Fail {
                        message: format!("Could not run checkupdates: {}", e),
                        fixable: false,
                    }
                }
            }
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some(
            "Install pacman-contrib if needed and update system packages with pacman -Syu"
                .to_string(),
        )
    }

    async fn fix(&self) -> Result<()> {
        use crate::common::requirements::PACMAN_CONTRIB_PACKAGE;

        // Ensure pacman-contrib is installed (provides checkupdates)
        if !PACMAN_CONTRIB_PACKAGE.is_installed()
            && !PACMAN_CONTRIB_PACKAGE.ensure()?.is_installed()
        {
            return Err(anyhow::anyhow!("pacman-contrib installation cancelled"));
        }

        // Run pacman -Syu
        let status = TokioCommand::new("pacman").arg("-Syu").status().await?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("pacman -Syu failed"))
        }
    }
}