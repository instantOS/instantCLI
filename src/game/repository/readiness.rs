//! Launch-time setup for the backup tools and this device's storage connection.

use anyhow::{Context, Result, bail};

use crate::common::deps::RESTIC;
use crate::common::package::{InstallResult, ensure_all};
use crate::game::config::InstantGameConfig;
use crate::game::utils::validation;

use super::rclone;

pub(crate) fn check(config: &InstantGameConfig) -> Result<()> {
    validation::check_restic_availability()?;
    if !config.is_initialized() {
        bail!("Game backup storage has not been set up. Run `ins game init`.");
    }
    rclone::check_configured(&config.repo.as_path().to_string_lossy())
}

/// Returns false on cancellation. Never creates a repository merely because a
/// connection failed; only the explicit initialization wizard can create one.
pub(crate) fn prepare() -> Result<bool> {
    let config = InstantGameConfig::load()?;
    let repository = config.repo.as_path().to_string_lossy();
    let mut dependencies = vec![&RESTIC];
    if rclone::repository_remote(&repository)?.is_some() {
        dependencies.push(&rclone::RCLONE);
    }
    match ensure_all(&dependencies)? {
        InstallResult::AlreadyInstalled | InstallResult::Installed => {}
        InstallResult::Declined => return Ok(false),
        InstallResult::NotAvailable { name, hint } => {
            bail!("{name} is required for game backups. {hint}")
        }
        InstallResult::Failed { reason } => bail!("Could not install backup tools: {reason}"),
    }
    validation::check_restic_availability()?;
    if !validation::prompt_initialize_if_needed()? {
        return Ok(false);
    }
    let config = InstantGameConfig::load()?;
    rclone::ensure_configured(&config.repo.as_path().to_string_lossy()).context(
        "Could not configure game backup storage. Run `ins game init` to review storage settings",
    )
}
