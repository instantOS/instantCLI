use serde::Deserialize;
use std::process::Command;

use crate::restic::error::ResticError;

#[derive(Debug, Clone)]
pub struct ResticWrapper {
    repository: String,
    password: String,
}

impl ResticWrapper {
    pub fn new(repository: String, password: String) -> Self {
        Self {
            repository,
            password,
        }
    }

    fn base_command(&self) -> Command {
        let mut cmd = Command::new("restic");
        cmd.arg("-r")
            .arg(&self.repository)
            .env("RESTIC_PASSWORD", &self.password);
        cmd
    }

    pub fn repository_exists(&self) -> Result<bool, ResticError> {
        let output = self.base_command().args(["cat", "config"]).output()?;

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
        let output = self.base_command().arg("version").output()?;

        Ok(output.status.success())
    }

    pub fn init_repository(&self) -> Result<(), ResticError> {
        let output = self.base_command().args(["init"]).output()?;

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

        let output = self.base_command().args(&args).output()?;

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
        let output = self.base_command().args(["snapshots", "--json"]).output()?;

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

        let output = self.base_command().args(&args).output()?;

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
        let output = self
            .base_command()
            .args([
                "restore",
                snapshot_id,
                "--target",
                target_path.to_str().ok_or_else(|| {
                    ResticError::CommandFailed(format!("Invalid target path: {target_path:?}"))
                })?,
                "--json",
            ])
            .output()?;

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
            let value: serde_json::Value = serde_json::from_str(line)?;

            if let Some(msg_type) = value.get("message_type") {
                match msg_type.as_str() {
                    Some("summary") => {
                        summary = Some(serde_json::from_value(value)?);
                    }
                    Some("error") => {
                        errors.push(serde_json::from_value(value)?);
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
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
    pub data_added: u64,
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BackupError {
    pub error: BackupErrorDetails,
}

#[derive(Debug, Deserialize)]
pub struct BackupErrorDetails {
    pub message: String,
    pub during: String,
    pub item: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Snapshot {
    pub time: String,
    pub id: String,
    pub short_id: String,
    pub paths: Vec<String>,
    pub hostname: String,
    pub username: String,
    pub tags: Vec<String>,
    pub summary: Option<SnapshotSummary>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SnapshotSummary {
    pub backup_start: String,
    pub backup_end: String,
    pub files_new: u64,
    pub files_changed: u64,
    pub files_unmodified: u64,
    pub data_added: u64,
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
            let value: serde_json::Value = serde_json::from_str(line)?;

            if let Some(msg_type) = value.get("message_type") {
                match msg_type.as_str() {
                    Some("summary") => {
                        summary = Some(serde_json::from_value(value)?);
                    }
                    Some("error") => {
                        errors.push(serde_json::from_value(value)?);
                    }
                    _ => {}
                }
            }
        }

        Ok(RestoreProgress { summary, errors })
    }
}

#[derive(Debug, Deserialize)]
pub struct RestoreSummary {
    pub total_files: u64,
    pub files_restored: u64,
    pub files_skipped: u64,
    pub total_bytes: u64,
    pub bytes_restored: u64,
}

#[derive(Debug, Deserialize)]
pub struct RestoreError {
    pub error: RestoreErrorDetails,
}

#[derive(Debug, Deserialize)]
pub struct RestoreErrorDetails {
    pub message: String,
    pub item: Option<String>,
}
