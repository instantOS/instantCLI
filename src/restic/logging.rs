use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use chrono::{DateTime, Utc};
use anyhow::{Context, Result};

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
        create_dir_all(&log_dir)
            .context("Failed to create restic log directory")?;

        Ok(Self { log_dir })
    }

    fn get_log_dir() -> Result<PathBuf> {
        let mut path = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;
        path.push("instantos");
        path.push("restic_logs");
        Ok(path)
    }

    pub fn log_command(
        &self,
        command: &str,
        args: &[String],
        output: &std::process::Output,
        repository: &str,
    ) -> Result<()> {
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

        let json_line = serde_json::to_string(&log_entry)
            .context("Failed to serialize restic command log")?;

        writeln!(file, "{}", json_line)
            .context("Failed to write to restic log file")?;

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

        let content = std::fs::read_to_string(&log_file)
            .context("Failed to read restic log file")?;

        let mut logs = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let log: ResticCommandLog = serde_json::from_str(line)
                .context("Failed to parse restic log entry")?;
            logs.push(log);
        }

        // Return logs in chronological order (newest first)
        logs.reverse();
        Ok(logs)
    }

    pub fn clear_logs(&self) -> Result<()> {
        let log_file = self.get_log_file_path();
        if log_file.exists() {
            std::fs::remove_file(&log_file)
                .context("Failed to remove restic log file")?;
        }
        Ok(())
    }

    pub fn print_recent_logs(&self, limit: Option<usize>) -> Result<()> {
        let logs = self.get_logs()?;
        let limit = limit.unwrap_or(10);
        let recent_logs = logs.iter().take(limit);

        println!("📋 Recent Restic Command Logs:");
        println!("{}", "═".repeat(80));

        for (i, log) in recent_logs.enumerate() {
            println!("Log Entry #{}", i + 1);
            println!("  🕒 Time: {}", log.timestamp.format("%Y-%m-%d %H:%M:%S UTC"));
            println!("  📝 Command: {} {}", log.command, log.args.join(" "));
            println!("  📁 Repository: {}", log.repository);
            println!("  ✅ Success: {}", if log.success { "Yes" } else { "No" });
            if let Some(code) = log.exit_code {
                println!("  🏁 Exit Code: {}", code);
            }

            if !log.stdout.trim().is_empty() {
                println!("  📤 STDOUT:\n{}", Self::indent_text(&log.stdout, 4));
            }

            if !log.stderr.trim().is_empty() {
                println!("  📥 STDERR:\n{}", Self::indent_text(&log.stderr, 4));
            }

            println!("{}", "═".repeat(80));
        }

        if logs.is_empty() {
            println!("No restic command logs found.");
        }

        Ok(())
    }

    fn indent_text(text: &str, indent: usize) -> String {
        let indent_str = " ".repeat(indent);
        text.lines()
            .map(|line| format!("{}{}", indent_str, line))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for ResticCommandLogger {
    fn default() -> Self {
        Self::new().expect("Failed to create ResticCommandLogger")
    }
}