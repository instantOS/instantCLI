use anyhow::{Context, Result};
use std::process::Command;

use crate::game::config::InstantGameConfig;

/// Execute restic commands with proper environment and configuration
pub struct ResticCommand {
    config: InstantGameConfig,
}

impl ResticCommand {
    pub fn new(config: InstantGameConfig) -> Self {
        Self { config }
    }

    /// Create a base restic command with repository and password
    fn base_command(&self) -> Result<Command> {
        let mut cmd = Command::new("restic");

        // Set repository
        cmd.arg("-r")
           .arg(self.config.repo.as_path());

        // Set password via environment variable
        cmd.env("RESTIC_PASSWORD", &self.config.repo_password);

        Ok(cmd)
    }

    /// Initialize restic repository
    pub fn init(&self) -> Result<()> {
        let mut cmd = self.base_command()?;
        cmd.arg("init");

        let output = cmd.output()
            .context("Failed to execute restic init")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Restic init failed: {}", stderr));
        }

        Ok(())
    }

    /// Check if repository is initialized
    pub fn check_repo(&self) -> Result<bool> {
        let mut cmd = self.base_command()?;
        cmd.arg("snapshots");
        cmd.arg("--json");

        let output = cmd.output()
            .context("Failed to execute restic snapshots")?;

        Ok(output.status.success())
    }

    /// Get repository status
    pub fn stats(&self) -> Result<String> {
        let mut cmd = self.base_command()?;
        cmd.arg("stats");
        cmd.arg("--json");

        let output = cmd.output()
            .context("Failed to execute restic stats")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Restic stats failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// List snapshots with optional filtering
    pub fn list_snapshots(&self, tags: Option<Vec<&str>>) -> Result<String> {
        let mut cmd = self.base_command()?;
        cmd.arg("snapshots");
        cmd.arg("--json");

        if let Some(tags) = tags {
            for tag in tags {
                cmd.arg("--tag");
                cmd.arg(tag);
            }
        }

        let output = cmd.output()
            .context("Failed to execute restic snapshots")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Restic snapshots failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}