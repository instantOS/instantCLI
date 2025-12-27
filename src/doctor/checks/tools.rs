use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

#[derive(Default)]
pub struct BatCheck;

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
