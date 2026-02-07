//! Shell completion check - verifies bash/zsh completions are installed and loaded
//!
//! This check uses the interactive subshell approach to spawn a bash/zsh subshell
//! and verify that completions for the `ins` command are properly registered.

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

/// Detected shell type
enum DetectedShell {
    Bash,
    Zsh,
    Other(String),
}

impl DetectedShell {
    /// Detect the current shell from the SHELL environment variable
    fn detect() -> Self {
        match std::env::var("SHELL") {
            Ok(shell_path) => {
                let shell_name = Path::new(&shell_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                match shell_name {
                    "bash" => Self::Bash,
                    "zsh" => Self::Zsh,
                    other => Self::Other(other.to_string()),
                }
            }
            Err(_) => Self::Other("unknown".to_string()),
        }
    }
}

#[derive(Default)]
pub struct ShellCompletionCheck;

#[async_trait]
impl DoctorCheck for ShellCompletionCheck {
    fn name(&self) -> &'static str {
        "Shell Completions"
    }

    fn id(&self) -> &'static str {
        "shell-completions"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        let shell = DetectedShell::detect();

        match shell {
            DetectedShell::Other(shell_name) => CheckStatus::Skipped(format!(
                "Unsupported shell: {} (only bash and zsh are supported)",
                shell_name
            )),

            DetectedShell::Bash => self.check_bash_completions().await,

            DetectedShell::Zsh => self.check_zsh_completions().await,
        }
    }

    fn fix_message(&self) -> Option<String> {
        let shell = DetectedShell::detect();
        let bin_name = env!("CARGO_BIN_NAME");

        match shell {
            DetectedShell::Bash => Some(format!(
                "Add the following line to ~/.bashrc or ~/.bash_profile:\n\
                 \n  source <(COMPLETE=bash {})\n\
                 \nThen restart your shell or run: source ~/.bashrc",
                bin_name
            )),
            DetectedShell::Zsh => Some(format!(
                "Add the following line to ~/.zshrc:\n\
                 \n  source <(COMPLETE=zsh {})\n\
                 \nThen restart your shell or run: source ~/.zshrc",
                bin_name
            )),
            DetectedShell::Other(name) => Some(format!(
                "Completions are only supported for bash and zsh.\n\
                 \nCurrent shell: {}\n\
                 Supported shells: bash, zsh",
                name
            )),
        }
    }

    async fn fix(&self) -> Result<()> {
        // Cannot auto-fix - requires user to manually add to shell config
        Err(anyhow::anyhow!(
            "Please manually add the completion source line to your shell configuration file"
        ))
    }
}

impl ShellCompletionCheck {
    /// Check if bash completions are installed and loaded
    async fn check_bash_completions(&self) -> CheckStatus {
        // Spawn interactive bash subshell and check if completions are registered
        let output = match self
            .spawn_subshell_with_timeout("bash", "complete -p ins")
            .await
        {
            Ok(output) => output,
            Err(e) => return CheckStatus::Skipped(format!("Could not spawn bash subshell: {}", e)),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Check if completion is registered
        // Success: exit code 0 AND stdout contains the completion declaration
        if output.status.success() && stdout.contains("complete -F _clap_complete_ins ins") {
            CheckStatus::Pass("Bash completions are installed and loaded".to_string())
        } else if stderr.contains("no completion specification") {
            // Bash explicitly says no completion for this command
            CheckStatus::Fail {
                message: "Bash completions are not installed".to_string(),
                fixable: true,
            }
        } else if !output.status.success() {
            // Command failed but not explicitly "not found"
            CheckStatus::Warning {
                message: "Bash completions may not be loaded in current session".to_string(),
                fixable: true,
            }
        } else {
            // Edge case: exit code 0 but completion not found in output
            CheckStatus::Warning {
                message: "Bash completions status unclear (completion function not found)"
                    .to_string(),
                fixable: true,
            }
        }
    }

    /// Check if zsh completions are installed and loaded
    async fn check_zsh_completions(&self) -> CheckStatus {
        // Spawn interactive zsh subshell and check if completion function exists
        let output = match self
            .spawn_subshell_with_timeout("zsh", "which _clap_dynamic_completer_ins")
            .await
        {
            Ok(output) => output,
            Err(e) => return CheckStatus::Skipped(format!("Could not spawn zsh subshell: {}", e)),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check if completion function is defined
        // Success: exit code 0 AND stdout contains the function name
        if output.status.success() && stdout.contains("_clap_dynamic_completer_ins") {
            CheckStatus::Pass("Zsh completions are installed and loaded".to_string())
        } else if !output.status.success() {
            // Function not found
            CheckStatus::Fail {
                message: "Zsh completions are not installed".to_string(),
                fixable: true,
            }
        } else {
            // Edge case: exit code 0 but function not in output
            CheckStatus::Warning {
                message: "Zsh completions status unclear (completion function not found)"
                    .to_string(),
                fixable: true,
            }
        }
    }

    /// Spawn an interactive subshell with a timeout
    async fn spawn_subshell_with_timeout(
        &self,
        shell: &str,
        command: &str,
    ) -> Result<std::process::Output> {
        let duration = Duration::from_secs(5);

        let output = timeout(
            duration,
            TokioCommand::new(shell)
                .arg("-i") // Interactive shell (loads .bashrc/.zshrc)
                .arg("-c")
                .arg(command)
                .output(),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Shell subprocess timed out after {} seconds",
                duration.as_secs()
            )
        })?
        .map_err(|e| anyhow::anyhow!("Failed to spawn subshell: {}", e))?;

        Ok(output)
    }
}
