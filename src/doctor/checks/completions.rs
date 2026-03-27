//! Shell completion check - verifies bash/zsh completions are installed and loaded
//!
//! This check uses the interactive subshell approach to spawn a bash/zsh subshell
//! and verify that completions for the `ins` command are properly registered.

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;
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

fn shell_installed(shell: &str) -> bool {
    which::which(shell).is_ok()
}

#[derive(Default)]
pub struct ShellCompletionCheck;

#[derive(Default)]
pub struct ZshHealthCheck;

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

            DetectedShell::Bash => {
                if !shell_installed("bash") {
                    CheckStatus::Skipped("bash is not installed".to_string())
                } else {
                    self.check_bash_completions().await
                }
            }

            DetectedShell::Zsh => {
                if !shell_installed("zsh") {
                    CheckStatus::Skipped("zsh is not installed".to_string())
                } else {
                    self.check_zsh_completions().await
                }
            }
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
                "If Zsh startup prints 'command not found: compdef', clear broken completion cache files (~/.zcompdump*) and restart the shell.\n\
                 If ~/.zsh_history is corrupted, move it aside and let Zsh create a fresh one.\n\
                 \nIf completions are not installed yet, add the following line to ~/.zshrc:\n\
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
        Err(anyhow::anyhow!(
            "Please manually add the completion source line to your shell configuration file"
        ))
    }
}

impl ShellCompletionCheck {
    /// Check if bash completions are installed and loaded
    async fn check_bash_completions(&self) -> CheckStatus {
        // Spawn interactive bash subshell and check if completions are registered
        let output = match spawn_subshell_with_timeout("bash", "complete -p ins").await {
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
        // Spawn interactive zsh subshell and check if completion function exists.
        // Also inspect stderr for broken completion initialization such as
        // "zsh: command not found: compdef", which usually indicates a corrupt
        // ~/.zcompdump cache or startup ordering issue.
        let output = match spawn_subshell_with_timeout("zsh", "which _clap_dynamic_completer_ins")
            .await
        {
            Ok(output) => output,
            Err(e) => return CheckStatus::Skipped(format!("Could not spawn zsh subshell: {}", e)),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if stderr.contains("command not found: compdef") {
            CheckStatus::Skipped(
                "Zsh health is failing; run the dedicated zsh-health check".to_string(),
            )
        } else if output.status.success() && stdout.contains("_clap_dynamic_completer_ins") {
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
}

#[async_trait]
impl DoctorCheck for ZshHealthCheck {
    fn name(&self) -> &'static str {
        "Zsh Health"
    }

    fn id(&self) -> &'static str {
        "zsh-health"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        if !shell_installed("zsh") {
            return CheckStatus::Skipped("zsh is not installed".to_string());
        }

        let output = match spawn_subshell_with_timeout(
            "zsh",
            "whence -w compdef; (( ${+_comps} )) && echo _comps=set || echo _comps=unset",
        )
        .await
        {
            Ok(output) => output,
            Err(e) => return CheckStatus::Skipped(format!("Could not spawn zsh subshell: {}", e)),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let zero_byte_hint = match count_zero_byte_zcompdump_files().await {
            Ok(count) if count > 0 => format!(" Found {} zero-byte ~/.zcompdump* file(s).", count),
            _ => String::new(),
        };

        let completion_broken = stderr.contains("command not found: compdef")
            || stdout.contains("compdef: none")
            || stdout.contains("_comps=unset");
        let history_issue = detect_zsh_history_issue().await;

        if completion_broken || history_issue.is_some() {
            let mut problems = Vec::new();
            if completion_broken {
                problems.push(format!(
                    "completion init is broken (`compdef` or `_comps` missing).{}",
                    zero_byte_hint
                ));
            }
            if let Some(issue) = history_issue {
                problems.push(issue);
            }

            CheckStatus::Fail {
                message: format!("Zsh health issues detected: {}", problems.join(" ")),
                fixable: true,
            }
        } else {
            CheckStatus::Pass(
                "Zsh startup, completion cache, and history file look healthy".to_string(),
            )
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some(
            "Remove broken ~/.zcompdump cache files, move aside a corrupted ~/.zsh_history if present, and restart Zsh"
                .to_string(),
        )
    }

    async fn fix(&self) -> Result<()> {
        let removed = remove_zsh_compdump_files().await?;
        let moved_history = repair_zsh_history_file().await?;

        if removed == 0 && !moved_history {
            Err(anyhow::anyhow!(
                "No broken ~/.zcompdump* files or corrupted ~/.zsh_history were found"
            ))
        } else {
            Ok(())
        }
    }
}

fn zcompdump_candidates() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"));

    [
        ".zcompdump",
        ".zcompdump.dat",
        ".zcompdump.zwc",
        ".zcompdump.zwc.old",
    ]
    .into_iter()
    .map(|name| home.join(name))
    .collect()
}

async fn count_zero_byte_zcompdump_files() -> Result<usize> {
    let mut zero_byte_count = 0;

    for path in zcompdump_candidates() {
        if let Ok(metadata) = fs::metadata(&path).await
            && metadata.is_file()
            && metadata.len() == 0
        {
            zero_byte_count += 1;
        }
    }

    Ok(zero_byte_count)
}

async fn remove_zsh_compdump_files() -> Result<usize> {
    let mut removed = 0;

    for path in zcompdump_candidates() {
        match fs::remove_file(&path).await {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to remove {}: {}",
                    path.display(),
                    e
                ));
            }
        }
    }

    Ok(removed)
}

fn zsh_history_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".zsh_history"))
}

async fn detect_zsh_history_issue() -> Option<String> {
    let path = zsh_history_path()?;
    let bytes = fs::read(&path).await.ok()?;

    if bytes.is_empty() {
        return Some("~/.zsh_history is zero-byte".to_string());
    }

    if bytes.contains(&0) {
        return Some("~/.zsh_history contains NUL bytes and looks corrupted".to_string());
    }

    None
}

async fn repair_zsh_history_file() -> Result<bool> {
    let Some(path) = zsh_history_path() else {
        return Ok(false);
    };

    let Some(issue) = detect_zsh_history_issue().await else {
        return Ok(false);
    };

    let backup_path = path.with_extension(format!(
        "corrupt.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("Failed to compute backup timestamp: {}", e))?
            .as_secs()
    ));

    fs::rename(&path, &backup_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to move corrupted history file ({}): {}", issue, e))?;

    Ok(true)
}

/// Spawn an interactive subshell with a timeout
async fn spawn_subshell_with_timeout(shell: &str, command: &str) -> Result<std::process::Output> {
    let duration = Duration::from_secs(5);

    let output = timeout(
        duration,
        TokioCommand::new(shell)
            .arg("-i")
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
