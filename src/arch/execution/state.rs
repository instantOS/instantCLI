use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::disk::PreparedTarget;
use super::paths;
use super::step::InstallStep;

const STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallState {
    schema_version: u32,
    configuration_sha256: String,
    #[serde(default)]
    disk: Option<DiskPreparation>,
    completed_steps: BTreeSet<InstallStep>,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
enum DiskPreparation {
    InProgress,
    Prepared { target: PreparedTarget },
}

/// Why saved installation state cannot be resumed for this run.
///
/// Resuming is deliberately fail-closed: checkpoints from another
/// configuration, an incompatible state file, or an interrupted disk
/// preparation are never reused silently. The interactive installer turns this
/// into an explicit offer to start over; `arch exec` reports it verbatim.
#[derive(Debug)]
pub(crate) struct ResumeBlocked {
    reason: String,
}

impl ResumeBlocked {
    fn blocked(reason: impl Into<String>) -> anyhow::Error {
        anyhow::Error::new(Self {
            reason: reason.into(),
        })
    }
}

impl std::fmt::Display for ResumeBlocked {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for ResumeBlocked {}

impl InstallState {
    pub(super) fn new(configuration_sha256: &str) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            configuration_sha256: configuration_sha256.into(),
            disk: None,
            completed_steps: BTreeSet::new(),
            start_time: None,
        }
    }

    pub(super) fn lock() -> Result<File> {
        paths::ensure_host_state_dir()?;
        let path = paths::host_state_dir().join("install.lock");
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("Could not open installation lock {}", path.display()))?;
        file.try_lock()
            .map_err(anyhow::Error::from)
            .context("Could not acquire the installation lock. Check whether another installation is running")?;
        Ok(file)
    }

    pub(super) fn mark_start(&mut self) {
        if self.start_time.is_none() {
            self.start_time = Some(chrono::Utc::now());
        }
    }

    pub fn load() -> Result<Self> {
        Ok(Self::read(&paths::host_state_file())?.unwrap_or_else(|| Self::new("")))
    }

    fn read(path: &Path) -> Result<Option<Self>> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to read {}", path.display()));
            }
        };
        toml::from_str(&content)
            .map(Some)
            .with_context(|| format!("Invalid installation state at {}", path.display()))
    }

    pub(super) fn load_for_configuration(configuration_sha256: &str) -> Result<Self> {
        let path = paths::host_state_file();
        Self::resume(&path, configuration_sha256).with_context(|| {
            format!(
                "To deliberately start a fresh installation, remove {} and run the installer again",
                path.display()
            )
        })
    }

    fn resume(path: &Path, configuration_sha256: &str) -> Result<Self> {
        let Some(state) = Self::read(path)? else {
            return Ok(Self::new(configuration_sha256));
        };
        if state.schema_version != STATE_SCHEMA_VERSION {
            return Err(ResumeBlocked::blocked(
                "The saved installation state uses a different schema",
            ));
        }
        if state.configuration_sha256 != configuration_sha256 {
            return Err(ResumeBlocked::blocked(
                "The configuration differs from the saved installation",
            ));
        }
        if matches!(state.disk, Some(DiskPreparation::InProgress)) {
            return Err(ResumeBlocked::blocked(
                "Disk preparation was interrupted. Check the target disk before starting over",
            ));
        }
        Ok(state)
    }

    /// Remove saved execution state so the next attempt prepares the target
    /// from scratch.
    ///
    /// The target's own copy is removed too: `setup_chroot` only copies a
    /// checkpoint that exists, so a stale copy would let a later chroot
    /// re-entry resume work from the discarded attempt.
    pub(crate) fn discard() -> Result<()> {
        Self::discard_file(&paths::host_state_file())?;
        Self::discard_file(&paths::chroot_path(paths::STATE_FILE))
    }

    fn discard_file(path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("Could not remove {}", path.display()))
            }
        }
    }

    pub(super) fn save(&self) -> Result<()> {
        self.write(&paths::host_state_file())
    }

    fn write(&self, path: &Path) -> Result<()> {
        let dir = path
            .parent()
            .context("Missing installation state directory")?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        file.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path)
            .with_context(|| format!("Failed to save {}", path.display()))?;
        File::open(dir)?.sync_all()?;
        Ok(())
    }

    pub(super) fn begin_disk_preparation(&mut self) -> Result<()> {
        ensure!(self.disk.is_none(), "Disk preparation has already started");
        self.disk = Some(DiskPreparation::InProgress);
        self.save()
    }

    pub(super) fn record_prepared_target(&mut self, target: PreparedTarget) -> Result<()> {
        self.disk = Some(DiskPreparation::Prepared { target });
        self.save()
    }

    pub(super) fn prepared_target(&self) -> Option<&PreparedTarget> {
        match &self.disk {
            Some(DiskPreparation::Prepared { target }) => Some(target),
            _ => None,
        }
    }

    pub(super) fn mark_complete(&mut self, step: InstallStep) {
        self.completed_steps.insert(step);
    }

    pub(super) fn is_complete(&self, step: InstallStep) -> bool {
        if step == InstallStep::Disk {
            self.prepared_target().is_some()
        } else {
            self.completed_steps.contains(&step)
        }
    }

    pub(super) fn check_dependencies(&self, step: InstallStep) -> Result<(), Vec<InstallStep>> {
        let deps = step.dependencies();
        let missing: Vec<InstallStep> = deps
            .into_iter()
            .filter(|dep| !self.is_complete(*dep))
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_restart_interrupted_disk_preparation() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("install_state.toml");
        let mut state = InstallState::new("original");
        state.disk = Some(DiskPreparation::InProgress);
        state.write(&path)?;

        let error = InstallState::resume(&path, "original").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Disk preparation was interrupted")
        );
        assert!(InstallState::read(&path)?.is_some());
        Ok(())
    }

    #[test]
    fn only_resumes_the_configuration_that_prepared_the_target() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("install_state.toml");
        let mut state = InstallState::new("original");
        let target = toml::from_str(
            r#"
            [[mounts]]
            location = "root"
            filesystem = "ext4"
            [mounts.device]
            path = "/dev/vda2"
            uuid = "root-uuid"
            "#,
        )?;
        state.disk = Some(DiskPreparation::Prepared { target });
        state.mark_complete(InstallStep::Base);
        state.write(&path)?;

        let restored = InstallState::resume(&path, "original")?;
        assert!(restored.is_complete(InstallStep::Disk));
        assert!(restored.is_complete(InstallStep::Base));
        assert!(InstallState::resume(&path, "changed").is_err());
        assert!(InstallState::resume(&path, "original")?.is_complete(InstallStep::Disk));
        Ok(())
    }

    #[test]
    fn refusals_are_typed_so_the_installer_can_offer_to_start_over() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("install_state.toml");

        let mut state = InstallState::new("original");
        state.write(&path)?;
        let error = InstallState::resume(&path, "changed").unwrap_err();
        assert!(error.downcast_ref::<ResumeBlocked>().is_some());

        state.disk = Some(DiskPreparation::InProgress);
        state.write(&path)?;
        let error = InstallState::resume(&path, "original").unwrap_err();
        let blocked = error.downcast_ref::<ResumeBlocked>().unwrap();
        assert!(blocked.to_string().contains("interrupted"));
        Ok(())
    }

    #[test]
    fn discarding_removes_saved_state_and_tolerates_absence() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("install_state.toml");
        InstallState::new("original").write(&path)?;

        InstallState::discard_file(&path)?;
        assert!(!path.exists());

        // A second discard has nothing to remove and must not fail.
        InstallState::discard_file(&path)?;
        assert!(!path.exists());
        Ok(())
    }
}
