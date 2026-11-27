use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command as TokioCommand;

#[derive(Default)]
pub struct BatCheck;

#[derive(Default)]
pub struct FzfVersionCheck;

#[derive(Default)]
pub struct GitConfigCheck;

/// Minimum required fzf version (major, minor, patch)
const MIN_FZF_VERSION: (u32, u32, u32) = (0, 66, 0);

/// Parse fzf version string like "0.66.0 (debian)" into (major, minor, patch)
fn parse_fzf_version(version_output: &str) -> Option<(u32, u32, u32)> {
    // Version output format: "0.66.0 (debian)" or just "0.66.0"
    let version_part = version_output.split_whitespace().next()?;
    let parts: Vec<&str> = version_part.split('.').collect();
    if parts.len() >= 2 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
        Some((major, minor, patch))
    } else {
        None
    }
}

/// Compare two version tuples
fn version_meets_minimum(version: (u32, u32, u32), minimum: (u32, u32, u32)) -> bool {
    version >= minimum
}

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

#[async_trait]
impl DoctorCheck for FzfVersionCheck {
    fn name(&self) -> &'static str {
        "FZF Version"
    }

    fn id(&self) -> &'static str {
        "fzf-version"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        // Check if fzf is installed
        if which::which("fzf").is_err() {
            return CheckStatus::Fail {
                message: "fzf is not installed".to_string(),
                fixable: true,
            };
        }

        // Get fzf version
        let output = TokioCommand::new("fzf").arg("--version").output().await;

        match output {
            Ok(output) if output.status.success() => {
                let version_str = String::from_utf8_lossy(&output.stdout);
                if let Some(version) = parse_fzf_version(&version_str) {
                    if version_meets_minimum(version, MIN_FZF_VERSION) {
                        CheckStatus::Pass(format!(
                            "fzf {}.{}.{} meets minimum requirement",
                            version.0, version.1, version.2
                        ))
                    } else {
                        CheckStatus::Fail {
                            message: format!(
                                "fzf {}.{}.{} is too old (requires {}.{}.{}+)",
                                version.0,
                                version.1,
                                version.2,
                                MIN_FZF_VERSION.0,
                                MIN_FZF_VERSION.1,
                                MIN_FZF_VERSION.2
                            ),
                            fixable: true,
                        }
                    }
                } else {
                    CheckStatus::Warning {
                        message: format!("Could not parse fzf version: {}", version_str.trim()),
                        fixable: false,
                    }
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                CheckStatus::Fail {
                    message: format!("fzf --version failed: {}", stderr.trim()),
                    fixable: false,
                }
            }
            Err(e) => CheckStatus::Fail {
                message: format!("Failed to run fzf: {}", e),
                fixable: false,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some(
            "Install mise and use it to install fzf and other CLI tools (starship, zoxide, lazygit, delta)"
                .to_string(),
        )
    }

    async fn fix(&self) -> Result<()> {
        let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME not set"))?;
        let mise_path = format!("{}/.local/bin/mise", home);

        // Check if mise is installed
        let mise_installed = which::which("mise").is_ok() || Path::new(&mise_path).exists();

        if !mise_installed {
            // Install mise using the official installer
            println!("Installing mise...");
            let install_status = TokioCommand::new("sh")
                .arg("-c")
                .arg("curl https://mise.run | sh")
                .status()
                .await?;

            if !install_status.success() {
                return Err(anyhow::anyhow!("Failed to install mise"));
            }
            println!("mise installed successfully");
        }

        // Determine which mise binary to use
        let mise_bin = if which::which("mise").is_ok() {
            "mise".to_string()
        } else {
            mise_path
        };

        // Tools to install
        let tools = [
            "fzf@latest",
            "starship@latest",
            "zoxide@latest",
            "lazygit@latest",
            "delta@latest",
            "yazi@latest",
        ];

        for tool in tools {
            println!("Installing {}...", tool);
            let status = TokioCommand::new(&mise_bin)
                .args(["use", "-g", tool])
                .status()
                .await?;

            if !status.success() {
                return Err(anyhow::anyhow!("Failed to install {}", tool));
            }
        }

        println!("All tools installed successfully!");
        println!(
            "\nNote: You may need to restart your shell or run 'eval \"$(mise activate)\"' for the tools to be available."
        );

        Ok(())
    }
}
