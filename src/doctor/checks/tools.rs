use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command as TokioCommand;

#[derive(Default)]
pub struct BatCheck;

#[derive(Default)]
pub struct GitConfigCheck;

/// Check if we're running inside a container
fn is_container() -> bool {
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

#[async_trait]
impl DoctorCheck for GitConfigCheck {
    fn name(&self) -> &'static str {
        "Git Commit Configuration"
    }

    fn id(&self) -> &'static str {
        "git-config"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        // Skip in containers
        if is_container() {
            return CheckStatus::Skipped("Running in a container".to_string());
        }

        // Check if git is installed
        if which::which("git").is_err() {
            return CheckStatus::Skipped("git is not installed".to_string());
        }

        // Check user.name
        let name_output = TokioCommand::new("git")
            .args(["config", "--global", "user.name"])
            .output()
            .await;

        let has_name = match name_output {
            Ok(output) => output.status.success() && !output.stdout.is_empty(),
            Err(_) => false,
        };

        // Check user.email
        let email_output = TokioCommand::new("git")
            .args(["config", "--global", "user.email"])
            .output()
            .await;

        let has_email = match email_output {
            Ok(output) => output.status.success() && !output.stdout.is_empty(),
            Err(_) => false,
        };

        match (has_name, has_email) {
            (true, true) => CheckStatus::Pass("Git user name and email are configured".to_string()),
            (false, false) => CheckStatus::Fail {
                message: "Git user.name and user.email are not configured".to_string(),
                fixable: false,
            },
            (true, false) => CheckStatus::Fail {
                message: "Git user.email is not configured".to_string(),
                fixable: false,
            },
            (false, true) => CheckStatus::Fail {
                message: "Git user.name is not configured".to_string(),
                fixable: false,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Run 'git config --global user.name \"Your Name\"' and 'git config --global user.email \"you@example.com\"'".to_string())
    }
}

#[async_trait]
impl DoctorCheck for BatCheck {
    fn name(&self) -> &'static str {
        "Bat Cache Compatibility"
    }

    fn id(&self) -> &'static str {
        "bat-cache"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        // Check if bat is installed
        if which::which("bat").is_err() {
            return CheckStatus::Skipped("bat is not installed".to_string());
        }

        // Create a temporary file to run bat on
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("instant_doctor_bat_check");

        // Write a small content to the file just in case
        if let Err(e) = tokio::fs::write(&temp_file, "check").await {
            return CheckStatus::Fail {
                message: format!("Could not create temp file for bat check: {}", e),
                fixable: false,
            };
        }

        // Run bat on the file with paging disabled
        // We use --paging=never to avoid interactive mode
        let output = TokioCommand::new("bat")
            .arg("--paging=never")
            .arg(&temp_file)
            .output()
            .await;

        // Clean up temp file
        let _ = tokio::fs::remove_file(&temp_file).await;

        match output {
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Check for the specific error message
                if stderr.contains("binary caches") && stderr.contains("not compatible") {
                    return CheckStatus::Fail {
                        message: "Bat binary cache is incompatible with current version"
                            .to_string(),
                        fixable: true,
                    };
                }

                CheckStatus::Pass("Bat cache is valid".to_string())
            }
            Err(e) => CheckStatus::Fail {
                message: format!("Failed to run bat: {}", e),
                fixable: false,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Run 'bat cache --build' to rebuild the cache".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let status = TokioCommand::new("bat")
            .arg("cache")
            .arg("--build")
            .status()
            .await?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("'bat cache --build' failed"))
        }
    }
}
