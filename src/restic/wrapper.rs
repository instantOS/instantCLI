use serde::Deserialize;
use std::process::Command;

use crate::restic::error::ResticError;
use crate::restic::logging::ResticCommandLogger;

#[derive(Debug, Clone)]
pub struct ResticWrapper {
    repository: String,
    password: String,
    logger: ResticCommandLogger,
}

impl ResticWrapper {
    pub fn new(repository: String, password: String) -> Self {
        Self {
            repository,
            password,
            logger: ResticCommandLogger::new().expect("Failed to create restic command logger"),
        }
    }

    fn execute_and_log_command(
        &self,
        mut command: Command,
        args: &[String],
    ) -> Result<std::process::Output, ResticError> {
        let output = command.output().map_err(|e| {
            ResticError::CommandFailed(format!("Failed to execute restic command: {e}"))
        })?;

        // Log the command execution
        if let Err(e) = self.logger.log_command(
            &command.get_program().to_string_lossy(),
            args,
            &output,
            &self.repository,
        ) {
            eprintln!("Warning: Failed to log restic command: {e}");
        }

        Ok(output)
    }

    fn base_command(&self) -> Command {
        let mut cmd = Command::new("restic");
        cmd.arg("-r")
            .arg(&self.repository)
            .env("RESTIC_PASSWORD", &self.password);
        cmd
    }

    pub fn repository_exists(&self) -> Result<bool, ResticError> {
        let mut cmd = self.base_command();
        cmd.args(["cat", "config"]);
        let args = vec!["cat".to_string(), "config".to_string()];

        let output = self.execute_and_log_command(cmd, &args)?;

        match output.status.code() {
            Some(0) => Ok(true),
            Some(10) => Ok(false), // Repository doesn't exist
            Some(code) => {
                let stderr = String::from_utf8(output.stderr)?;
                Err(ResticError::from_exit_code(code, &stderr))
            }
            None => Err(ResticError::CommandFailed(
                "Process terminated by signal".to_string(),
            )),
        }
    }

    pub fn check_version(&self) -> Result<bool, ResticError> {
        let mut cmd = self.base_command();
        cmd.arg("version");
        let args = vec!["version".to_string()];

        let output = self.execute_and_log_command(cmd, &args)?;

        Ok(output.status.success())
    }

    pub fn init_repository(&self) -> Result<(), ResticError> {
        let mut cmd = self.base_command();
        cmd.args(["init"]);
        let args = vec!["init".to_string()];

        let output = self.execute_and_log_command(cmd, &args)?;

        if output.status.success() {
            Ok(())
        } else {
            let code = output.status.code().unwrap_or(1);
            let stderr = String::from_utf8(output.stderr)?;
            Err(ResticError::from_exit_code(code, &stderr))
        }
    }

    pub fn backup<P: AsRef<std::path::Path>>(
        &self,
        paths: &[P],
        tags: Vec<String>,
    ) -> Result<BackupProgress, ResticError> {
        // Ensure restic will skip creating a snapshot when nothing changed
        let mut args: Vec<String> = vec![
            "backup".to_string(),
            "--skip-if-unchanged".to_string(),
            "--json".to_string(),
        ];

        // Add required tags
        for tag in tags {
            args.push("--tag".to_string());
            args.push(tag);
        }

        for path in paths {
            args.push(
                path.as_ref()
                    .to_str()
                    .ok_or_else(|| {
                        ResticError::CommandFailed(format!("Invalid path: {:?}", path.as_ref()))
                    })?
                    .to_string(),
            );
        }

        let mut cmd = self.base_command();
        cmd.args(&args);
        let output = self.execute_and_log_command(cmd, &args)?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(1);
            let stderr = String::from_utf8(output.stderr)?;
            return Err(ResticError::from_exit_code(code, &stderr));
        }

        let stdout = String::from_utf8(output.stdout)?;
        BackupProgress::parse(&stdout)
    }

    pub fn list_snapshots(&self) -> Result<Vec<Snapshot>, ResticError> {
        // Deprecated simple listing: delegate to the filtered listing with no tags
        let mut cmd = self.base_command();
        cmd.args(["snapshots", "--json"]);
        let args = vec!["snapshots".to_string(), "--json".to_string()];

        let output = self.execute_and_log_command(cmd, &args)?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(1);
            let stderr = String::from_utf8(output.stderr)?;
            return Err(ResticError::from_exit_code(code, &stderr));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let snapshots: Vec<Snapshot> = serde_json::from_str(&stdout)?;
        Ok(snapshots)
    }

    /// List snapshots with optional tag filtering. Returns raw JSON output.
    pub fn list_snapshots_filtered(
        &self,
        tags: Option<Vec<String>>,
    ) -> Result<String, ResticError> {
        let mut args: Vec<String> = vec!["snapshots".to_string(), "--json".to_string()];

        if let Some(tags) = tags {
            for tag in tags {
                args.push("--tag".to_string());
                args.push(tag);
            }
        }

        let mut cmd = self.base_command();
        cmd.args(&args);
        let output = self.execute_and_log_command(cmd, &args)?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(1);
            let stderr = String::from_utf8(output.stderr)?;
            return Err(ResticError::from_exit_code(code, &stderr));
        }

        let stdout = String::from_utf8(output.stdout)?;
        Ok(stdout)
    }

    pub fn restore(
        &self,
        snapshot_id: &str,
        target_path: &std::path::Path,
    ) -> Result<RestoreProgress, ResticError> {
        let args = vec![
            "restore".to_string(),
            snapshot_id.to_string(),
            "--target".to_string(),
            target_path
                .to_str()
                .ok_or_else(|| {
                    ResticError::CommandFailed(format!("Invalid target path: {target_path:?}"))
                })?
                .to_string(),
            "--json".to_string(),
        ];

        let mut cmd = self.base_command();
        cmd.args(&args);
        let output = self.execute_and_log_command(cmd, &args)?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(1);
            let stderr = String::from_utf8(output.stderr)?;
            return Err(ResticError::from_exit_code(code, &stderr));
        }

        let stdout = String::from_utf8(output.stdout)?;
        RestoreProgress::parse(&stdout)
    }
}

#[derive(Debug, Deserialize)]
pub struct BackupProgress {
    pub summary: Option<BackupSummary>,
    pub errors: Vec<BackupError>,
}

impl BackupProgress {
    fn parse(output: &str) -> Result<Self, ResticError> {
        let mut summary = None;
        let mut errors = Vec::new();

        for line in output.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let value: serde_json::Value = serde_json::from_str(line)?;

            if let Some(msg_type) = value.get("message_type") {
                match msg_type.as_str() {
                    Some("summary") => {
                        summary = Some(serde_json::from_value(value)?);
                    }
                    Some("error") => {
                        // Error messages have nested structure according to docs
                        if let Some(error_msg) = value.get("error").and_then(|e| e.get("message")) {
                            let during = value
                                .get("during")
                                .and_then(|d| d.as_str())
                                .unwrap_or("backup")
                                .to_string();
                            let item = value
                                .get("item")
                                .and_then(|i| i.as_str())
                                .map(|s| s.to_string());

                            errors.push(BackupError {
                                message: error_msg.as_str().unwrap_or("Unknown error").to_string(),
                                during,
                                item,
                            });
                        } else {
                            // Fallback for different error structure
                            let message = value
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error")
                                .to_string();
                            let during = value
                                .get("during")
                                .and_then(|d| d.as_str())
                                .unwrap_or("backup")
                                .to_string();
                            let item = value
                                .get("item")
                                .and_then(|i| i.as_str())
                                .map(|s| s.to_string());

                            errors.push(BackupError {
                                message,
                                during,
                                item,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(BackupProgress { summary, errors })
    }
}

#[derive(Debug, Deserialize)]
pub struct BackupSummary {
    #[serde(default)]
    pub dry_run: bool,
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
    pub dirs_new: u64,
    pub dirs_changed: u64,
    pub dirs_unmodified: u64,
    pub data_blobs: i64,
    pub tree_blobs: i64,
    pub data_added: u64,
    pub data_added_packed: u64,
    pub total_files_processed: u64,
    pub total_bytes_processed: u64,
    pub backup_start: String,
    pub backup_end: String,
    pub total_duration: f64,
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BackupError {
    pub message: String,
    pub during: String,
    pub item: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Snapshot {
    pub time: String,
    pub parent: Option<String>,
    pub tree: String,
    pub paths: Vec<String>,
    pub hostname: String,
    pub username: String,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub excludes: Option<Vec<String>>,
    pub tags: Vec<String>,
    pub program_version: Option<String>,
    pub summary: Option<SnapshotSummary>,
    pub id: String,
    pub short_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SnapshotSummary {
    pub backup_start: String,
    pub backup_end: String,
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
    pub dirs_new: u64,
    pub dirs_changed: u64,
    pub dirs_unmodified: u64,
    pub data_blobs: i64,
    pub tree_blobs: i64,
    pub data_added: u64,
    pub data_added_packed: u64,
    pub total_files_processed: u64,
    pub total_bytes_processed: u64,
}

#[derive(Debug, Deserialize)]
pub struct RestoreProgress {
    pub summary: Option<RestoreSummary>,
    pub errors: Vec<RestoreError>,
}

impl RestoreProgress {
    fn parse(output: &str) -> Result<Self, ResticError> {
        let mut summary = None;
        let mut errors = Vec::new();

        for line in output.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let value: serde_json::Value = serde_json::from_str(line)?;

            if let Some(msg_type) = value.get("message_type") {
                match msg_type.as_str() {
                    Some("summary") => {
                        summary = Some(serde_json::from_value(value)?);
                    }
                    Some("error") => {
                        // Error messages have nested structure according to docs
                        if let Some(error_msg) = value.get("error").and_then(|e| e.get("message")) {
                            let during = value
                                .get("during")
                                .and_then(|d| d.as_str())
                                .unwrap_or("restore")
                                .to_string();
                            let item = value
                                .get("item")
                                .and_then(|i| i.as_str())
                                .map(|s| s.to_string());

                            errors.push(RestoreError {
                                message: error_msg.as_str().unwrap_or("Unknown error").to_string(),
                                during,
                                item,
                            });
                        } else {
                            // Fallback for different error structure
                            let message = value
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error")
                                .to_string();
                            let during = value
                                .get("during")
                                .and_then(|d| d.as_str())
                                .unwrap_or("restore")
                                .to_string();
                            let item = value
                                .get("item")
                                .and_then(|i| i.as_str())
                                .map(|s| s.to_string());

                            errors.push(RestoreError {
                                message,
                                during,
                                item,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(RestoreProgress { summary, errors })
    }
}

#[derive(Debug, Deserialize)]
//TODO: should all of these have a default value? Is this even necessary?
pub struct RestoreSummary {
    #[serde(default)]
    pub seconds_elapsed: u64,
    #[serde(default)]
    pub total_files: u64,
    #[serde(default)]
    pub files_restored: u64,
    #[serde(default)]
    pub files_skipped: u64,
    #[serde(default)]
    pub files_deleted: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub bytes_restored: u64,
    #[serde(default)]
    pub bytes_skipped: u64,
}

#[derive(Debug, Deserialize)]
pub struct RestoreError {
    pub message: String,
    pub during: String,
    pub item: Option<String>,
}
