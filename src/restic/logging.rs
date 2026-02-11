use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::PathBuf;

use crate::common::paths;

#[derive(Debug, Serialize, Deserialize)]
pub struct ResticCommandLog {
    pub timestamp: DateTime<Utc>,
    pub command: String,
    pub args: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub repository: String,
}

#[derive(Debug, Clone)]
pub struct ResticCommandLogger {
    log_dir: PathBuf,
}

impl ResticCommandLogger {
    pub fn new() -> Result<Self> {
        let log_dir = Self::get_log_dir()?;

        // Only create directory if debug is enabled
        if crate::ui::is_debug_enabled() {
            create_dir_all(&log_dir).context("Failed to create restic log directory")?;
        }

        Ok(Self { log_dir })
    }

    fn get_log_dir() -> Result<PathBuf> {
        paths::instant_restic_logs_dir()
    }

    pub fn log_command(
        &self,
        command: &str,
        args: &[String],
        output: &std::process::Output,
        repository: &str,
    ) -> Result<()> {
        // Skip logging if debug is not enabled
        if !crate::ui::is_debug_enabled() {
            return Ok(());
        }

        let log_entry = ResticCommandLog {
            timestamp: Utc::now(),
            command: command.to_string(),
            args: args.to_vec(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
            success: output.status.success(),
            repository: repository.to_string(),
        };

        let log_file = self.get_log_file_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .context("Failed to open restic log file")?;

        let json_line =
            serde_json::to_string(&log_entry).context("Failed to serialize restic command log")?;

        writeln!(file, "{json_line}").context("Failed to write to restic log file")?;

        Ok(())
    }

    fn get_log_file_path(&self) -> PathBuf {
        let mut log_file = self.log_dir.clone();
        log_file.push("restic_commands.jsonl");
        log_file
    }

    pub fn get_logs(&self) -> Result<Vec<ResticCommandLog>> {
        let log_file = self.get_log_file_path();
        if !log_file.exists() {
            return Ok(Vec::new());
        }

        let content =
            std::fs::read_to_string(&log_file).context("Failed to read restic log file")?;

        let mut logs = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let log: ResticCommandLog =
                serde_json::from_str(line).context("Failed to parse restic log entry")?;
            logs.push(log);
        }

        // Return logs in chronological order (newest first)
        logs.reverse();
        Ok(logs)
    }

    pub fn clear_logs(&self) -> Result<()> {
        let log_file = self.get_log_file_path();
        if log_file.exists() {
            std::fs::remove_file(&log_file).context("Failed to remove restic log file")?;
        }
        Ok(())
    }

    pub fn print_recent_logs(&self, limit: Option<usize>) -> Result<()> {
        use crate::ui::prelude::*;

        let logs = self.get_logs()?;
        let limit = limit.unwrap_or(10);
        let recent_logs = logs.iter().take(limit);

        emit(
            Level::Info,
            "restic.logs.list.start",
            &format!("{} Recent Restic Command Logs:", char::from(NerdFont::List)),
            None,
        );
        separator(false);

        for (i, log) in recent_logs.enumerate() {
            let time_str = log.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string();
            let mut text_block = String::new();
            text_block.push_str(&format!("Log Entry #{}\n", i + 1));
            text_block.push_str(&format!(
                "  {} Time: {}\n",
                char::from(NerdFont::Clock),
                time_str
            ));
            text_block.push_str(&format!(
                "   Command: {} {}\n",
                log.command,
                log.args.join(" ")
            ));
            text_block.push_str(&format!(
                "  {} Repository: {}\n",
                char::from(NerdFont::Folder),
                log.repository
            ));
            text_block.push_str(&format!(
                "  {} Success: {}\n",
                if log.success {
                    char::from(NerdFont::Check)
                } else {
                    char::from(NerdFont::CrossCircle)
                },
                if log.success { "Yes" } else { "No" }
            ));
            if let Some(code) = log.exit_code {
                text_block.push_str(&format!("   Exit Code: {}\n", code));
            }
            if !log.stdout.trim().is_empty() {
                text_block.push_str(&format!(
                    "  {} STDOUT:\n{}\n",
                    char::from(NerdFont::Upload),
                    Self::indent_text(&log.stdout, 4)
                ));
            }
            if !log.stderr.trim().is_empty() {
                text_block.push_str(&format!(
                    "  {} STDERR:\n{}\n",
                    char::from(NerdFont::Download),
                    Self::indent_text(&log.stderr, 4)
                ));
            }

            let data = serde_json::json!({
                "index": i + 1,
                "timestamp": time_str,
                "command": log.command,
                "args": log.args,
                "repository": log.repository,
                "success": log.success,
                "exit_code": log.exit_code,
                "stdout": log.stdout,
                "stderr": log.stderr,
            });
            emit(Level::Info, "restic.logs.entry", &text_block, Some(data));
            separator(false);
        }

        if logs.is_empty() {
            emit(
                Level::Info,
                "restic.logs.empty",
                &format!(
                    "{} No restic command logs found.",
                    char::from(NerdFont::Info)
                ),
                None,
            );
        }

        Ok(())
    }

    fn indent_text(text: &str, indent: usize) -> String {
        let indent_str = " ".repeat(indent);
        text.lines()
            .map(|line| format!("{indent_str}{line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for ResticCommandLogger {
    fn default() -> Self {
        Self::new().expect("Failed to create ResticCommandLogger")
    }
}
