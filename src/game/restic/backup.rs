use anyhow::{Context, Result};
use std::path::Path;

use crate::game::config::{InstantGameConfig, GameInstallation};

/// Backup game saves to restic repository with proper tagging
pub struct GameBackup {
    config: InstantGameConfig,
}

impl GameBackup {
    pub fn new(config: InstantGameConfig) -> Self {
        Self { config }
    }

    /// Create a backup of a specific game's save directory
    pub fn backup_game(&self, game_installation: &GameInstallation) -> Result<String> {
        // Validate that save path exists
        if !game_installation.save_path.as_path().exists() {
            return Err(anyhow::anyhow!(
                "Save path does not exist: {}",
                game_installation.save_path.as_path().display()
            ));
        }

        // Build restic backup command
        let mut cmd = std::process::Command::new("restic");

        // Set repository
        cmd.arg("-r")
           .arg(self.config.repo.as_path());

        // Set password via environment variable
        cmd.env("RESTIC_PASSWORD", &self.config.repo_password);

        // Backup command
        cmd.arg("backup");

        // Add tags: instantgame + game name
        cmd.arg("--tag");
        cmd.arg("instantgame");
        cmd.arg("--tag");
        cmd.arg(&game_installation.game_name.0);

        // Add the save path
        cmd.arg(game_installation.save_path.as_path());

        // Execute the command
        let output = cmd.output()
            .context("Failed to execute restic backup")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Restic backup failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.to_string())
    }

    /// List backups for a specific game
    pub fn list_game_backups(&self, game_name: &str) -> Result<String> {
        let mut cmd = std::process::Command::new("restic");

        // Set repository
        cmd.arg("-r")
           .arg(self.config.repo.as_path());

        // Set password via environment variable
        cmd.env("RESTIC_PASSWORD", &self.config.repo_password);

        // List snapshots with game-specific tags
        cmd.arg("snapshots");
        cmd.arg("--json");
        cmd.arg("--tag");
        cmd.arg("instantgame");
        cmd.arg("--tag");
        cmd.arg(game_name);

        let output = cmd.output()
            .context("Failed to execute restic snapshots")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Restic snapshots failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Restore a game backup
    pub fn restore_game_backup(&self, _game_name: &str, snapshot_id: &str, target_path: &Path) -> Result<String> {
        let mut cmd = std::process::Command::new("restic");

        // Set repository
        cmd.arg("-r")
           .arg(self.config.repo.as_path());

        // Set password via environment variable
        cmd.env("RESTIC_PASSWORD", &self.config.repo_password);

        // Restore command
        cmd.arg("restore");
        cmd.arg(snapshot_id);
        cmd.arg("--target");
        cmd.arg(target_path);

        let output = cmd.output()
            .context("Failed to execute restic restore")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Restic restore failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.to_string())
    }

    /// Check if restic is available on the system
    pub fn check_restic_availability() -> Result<bool> {
        let output = std::process::Command::new("restic")
            .arg("version")
            .output();

        match output {
            Ok(output) => Ok(output.status.success()),
            Err(_) => Ok(false),
        }
    }
}