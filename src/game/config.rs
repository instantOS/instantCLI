use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

use crate::common::TildePath;
use crate::common::config::DocumentedConfig;
use crate::common::paths;
use crate::game::launch_command::LaunchCommand;
use crate::restic::wrapper::Snapshot;

/// Describes what kind of filesystem element a tracked path represents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PathContentKind {
    #[default]
    Directory,
    File,
}

impl PathContentKind {
    pub fn is_file(self) -> bool {
        matches!(self, PathContentKind::File)
    }

    pub fn is_directory(self) -> bool {
        matches!(self, PathContentKind::Directory)
    }
}

impl From<std::fs::Metadata> for PathContentKind {
    fn from(metadata: std::fs::Metadata) -> Self {
        if metadata.is_file() {
            PathContentKind::File
        } else {
            PathContentKind::Directory
        }
    }
}

/// Configurable restic retention policy values stored in games.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct RetentionPolicyConfig {
    pub keep_last: Option<u32>,
    pub keep_daily: Option<u32>,
    pub keep_weekly: Option<u32>,
    pub keep_monthly: Option<u32>,
    pub keep_yearly: Option<u32>,
}

impl RetentionPolicyConfig {
    pub const DEFAULT_KEEP_DAILY: u32 = 90;
    pub const DEFAULT_KEEP_WEEKLY: u32 = 52;
    pub const DEFAULT_KEEP_MONTHLY: u32 = 36;
    pub const DEFAULT_KEEP_YEARLY: u32 = 10;

    pub fn is_default(value: &Self) -> bool {
        value.keep_last.is_none()
            && value.keep_daily.is_none()
            && value.keep_weekly.is_none()
            && value.keep_monthly.is_none()
            && value.keep_yearly.is_none()
    }

    pub fn effective(&self) -> RetentionPolicyValues {
        RetentionPolicyValues {
            keep_last: self.keep_last,
            keep_daily: self.keep_daily.unwrap_or(Self::DEFAULT_KEEP_DAILY),
            keep_weekly: self.keep_weekly.unwrap_or(Self::DEFAULT_KEEP_WEEKLY),
            keep_monthly: self.keep_monthly.unwrap_or(Self::DEFAULT_KEEP_MONTHLY),
            keep_yearly: self.keep_yearly.unwrap_or(Self::DEFAULT_KEEP_YEARLY),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicyValues {
    pub keep_last: Option<u32>,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
    pub keep_yearly: u32,
}

impl RetentionPolicyValues {
    pub fn to_rules(self) -> Vec<(String, String)> {
        let mut rules = Vec::with_capacity(5);

        if let Some(keep_last) = self.keep_last {
            rules.push(("keep-last".to_string(), keep_last.to_string()));
        }

        rules.push(("keep-daily".to_string(), self.keep_daily.to_string()));
        rules.push(("keep-weekly".to_string(), self.keep_weekly.to_string()));
        rules.push(("keep-monthly".to_string(), self.keep_monthly.to_string()));
        rules.push(("keep-yearly".to_string(), self.keep_yearly.to_string()));

        rules
    }
}

/// Wrapper type for game names
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameName(pub String);

impl std::fmt::Display for GameName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for GameName {
    fn from(s: String) -> Self {
        GameName(s)
    }
}

impl From<&str> for GameName {
    fn from(s: &str) -> Self {
        GameName(s.to_string())
    }
}

/// Portable game declarations; copying them between devices is explicit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub name: GameName,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_group: Option<String>,
    pub launch_command: Option<LaunchCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<GameDependency>,
}

impl Game {
    pub fn new(name: impl Into<GameName>) -> Self {
        Self {
            name: name.into(),
            description: None,
            save_group: None,
            launch_command: None,
            dependencies: Vec::new(),
        }
    }
}

/// Definition of a game dependency stored in games.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDependency {
    pub id: String,
    pub source_path: String,
    #[serde(default)]
    pub source_type: PathContentKind,
}

/// Data owned by either a private game or a shared save group.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedSaves {
    pub save_path: TildePath,
    #[serde(default)]
    pub save_path_type: PathContentKind,
    pub nearest_checkpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_time: Option<String>,
    /// Repository associated with this installation's local sync state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_repository: Option<String>,
    /// Remote head acknowledged by an explicit historical restore, not local
    /// contents. Never the restored snapshot itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_snapshot: Option<String>,
    /// Snapshot whose restore must complete before automatic sync is safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_restore: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<SaveBaseline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveBaseline {
    pub content: String,
    pub remote_head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSaveGroup {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveGroupBinding {
    pub id: String,
    #[serde(flatten)]
    pub saves: OwnedSaves,
}

/// Device-specific launch overrides and, for private games, inline save ownership.
#[derive(Debug, Clone, Serialize)]
pub struct GameInstallation {
    pub game_name: GameName,
    #[serde(flatten)]
    pub saves: Option<OwnedSaves>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_command: Option<LaunchCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<InstalledDependency>,
}

impl<'de> Deserialize<'de> for GameInstallation {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            game_name: GameName,
            launch_command: Option<LaunchCommand>,
            #[serde(default)]
            dependencies: Vec<InstalledDependency>,
            #[serde(flatten)]
            saves: toml::Table,
        }
        let raw = Raw::deserialize(deserializer)?;
        let saves = if raw.saves.is_empty() {
            None
        } else {
            Some(
                toml::Value::Table(raw.saves)
                    .try_into()
                    .map_err(serde::de::Error::custom)?,
            )
        };
        Ok(Self {
            game_name: raw.game_name,
            saves,
            launch_command: raw.launch_command,
            dependencies: raw.dependencies,
        })
    }
}

impl GameInstallation {
    pub fn with_kind(
        game_name: impl Into<GameName>,
        save_path: impl Into<TildePath>,
        kind: PathContentKind,
    ) -> Self {
        Self {
            game_name: game_name.into(),
            saves: Some(OwnedSaves::new(save_path, kind)),
            launch_command: None,
            dependencies: Vec::new(),
        }
    }

    pub fn owned(&self) -> Result<&OwnedSaves> {
        self.saves
            .as_ref()
            .context("This game uses shared saves; operate on its save group")
    }

    pub fn owned_mut(&mut self) -> Result<&mut OwnedSaves> {
        self.saves
            .as_mut()
            .context("This game uses shared saves; operate on its save group")
    }
}

impl OwnedSaves {
    pub fn new(save_path: impl Into<TildePath>, kind: PathContentKind) -> Self {
        Self {
            save_path: save_path.into(),
            save_path_type: kind,
            nearest_checkpoint: None,
            checkpoint_time: None,
            sync_repository: None,
            acknowledged_snapshot: None,
            pending_restore: None,
            baseline: None,
        }
    }

    /// Record a fresh backup as the checkpoint local saves reflect.
    pub fn note_backup(&mut self, checkpoint_id: impl Into<String>) {
        self.set_checkpoint(checkpoint_id, chrono::Utc::now().to_rfc3339());
    }

    /// Settle a completed restore in one transition.
    ///
    /// The checkpoint records what local saves now contain; the acknowledged
    /// head records which remote head was deliberately not restored. The two
    /// must differ: acknowledging a head equal to the checkpoint would claim
    /// the local saves hold contents they do not.
    pub fn note_restored_snapshot(
        &mut self,
        checkpoint_id: impl Into<String>,
        time: impl Into<String>,
        acknowledged_head: Option<String>,
    ) {
        let checkpoint_id = checkpoint_id.into();
        debug_assert!(
            acknowledged_head.as_deref() != Some(checkpoint_id.as_str()),
            "an acknowledged head must differ from the restored checkpoint"
        );
        self.set_checkpoint(checkpoint_id, time);
        self.acknowledged_snapshot = acknowledged_head;
    }

    /// Every checkpoint transition invalidates transient sync state: the
    /// previous acknowledgement and any pending-restore marker describe the
    /// superseded state, not the new one.
    fn set_checkpoint(&mut self, checkpoint_id: impl Into<String>, time: impl Into<String>) {
        self.nearest_checkpoint = Some(checkpoint_id.into());
        self.checkpoint_time = Some(time.into());
        self.acknowledged_snapshot = None;
        self.pending_restore = None;
    }

    /// Legacy installations without an associated repository remain compatible.
    pub fn needs_repository_reconciliation(&self, repository: &str) -> bool {
        self.sync_repository
            .as_deref()
            .is_some_and(|previous| previous != repository)
    }

    /// Whether `snapshot` is this installation's checkpoint.
    ///
    /// Matching tolerates the short form because configs written before
    /// checkpoints were normalized to full IDs (or hand-edited with the short
    /// form `game snapshots` displays) must still match — a mismatch here makes
    /// sync re-restore over local saves.
    pub fn checkpoint_matches(&self, snapshot: &Snapshot) -> bool {
        self.nearest_checkpoint
            .as_deref()
            .is_some_and(|checkpoint| snapshot.matches_id(checkpoint))
    }
}

/// Installed dependency mapping stored in installations.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledDependency {
    pub dependency_id: String,
    pub install_path: TildePath,
    #[serde(default)]
    pub install_path_type: PathContentKind,
}

/// Main game configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstantGameConfig {
    #[serde(skip)]
    pub revision: std::cell::RefCell<Option<String>>,
    #[serde(default = "InstantGameConfig::default_repo")]
    pub repo: TildePath,
    #[serde(default = "InstantGameConfig::default_password")]
    pub repo_password: String,
    #[serde(default)]
    pub games: Vec<Game>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_groups: Vec<SharedSaveGroup>,
    #[serde(default, skip_serializing_if = "RetentionPolicyConfig::is_default")]
    pub retention_policy: RetentionPolicyConfig,
}

impl InstantGameConfig {
    fn default_repo() -> TildePath {
        TildePath::new(PathBuf::new())
    }

    fn default_password() -> String {
        "instantgamepassword".to_string()
    }
}

impl Default for InstantGameConfig {
    fn default() -> Self {
        Self {
            revision: Default::default(),
            repo: Self::default_repo(),
            repo_password: Self::default_password(),
            games: Vec::new(),
            save_groups: Vec::new(),
            retention_policy: RetentionPolicyConfig::default(),
        }
    }
}

/// Device-specific installations configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstallationsConfig {
    #[serde(skip)]
    pub revision: std::cell::RefCell<Option<String>>,
    #[serde(default)]
    pub installations: Vec<GameInstallation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_groups: Vec<SaveGroupBinding>,
}

pub fn games_config_path() -> Result<PathBuf> {
    Ok(paths::games_config_dir()?.join("games.toml"))
}

pub fn installations_config_path() -> Result<PathBuf> {
    Ok(paths::games_config_dir()?.join("installations.toml"))
}

impl InstantGameConfig {
    pub fn load() -> Result<Self> {
        let path = games_config_path()?;
        let config = <Self as DocumentedConfig>::load_from_path_documented(&path)?;
        *config.revision.borrow_mut() = config_revision(&path)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        super::ownership::validate_groups(self)?;
        let mut names = HashSet::new();
        for game in &self.games {
            if !names.insert(&game.name.0) {
                return Err(anyhow::anyhow!(
                    "Duplicate game name found: {}",
                    game.name.0
                ));
            }
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        save_atomic(
            self,
            &games_config_path()?,
            &self.revision,
            "Instant game configuration",
            true,
        )
    }

    pub fn is_initialized(&self) -> bool {
        !self.repo.as_path().as_os_str().is_empty()
    }
}

impl InstallationsConfig {
    pub fn load() -> Result<Self> {
        let path = installations_config_path()?;
        let config = <Self as DocumentedConfig>::load_from_path_documented(&path)?;
        *config.revision.borrow_mut() = config_revision(&path)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for installation in &self.installations {
            if !names.insert(&installation.game_name.0) {
                return Err(anyhow::anyhow!(
                    "Duplicate installation for game found: {}",
                    installation.game_name.0
                ));
            }
        }
        let mut groups = HashSet::new();
        for group in &self.save_groups {
            anyhow::ensure!(
                groups.insert(&group.id),
                "Duplicate local save group '{}'",
                group.id
            );
        }
        Ok(())
    }

    /// Apply one owner's state transition against the latest config while its
    /// file lock is held. Concurrent sessions for other owners cannot lose updates.
    pub(crate) fn update_owner(
        id: &super::ownership::OwnerId,
        update: impl FnOnce(&mut OwnedSaves) -> Result<()>,
    ) -> Result<()> {
        let path = installations_config_path()?;
        let _lock = super::save_runtime::SaveLock::at_wait(&path.with_extension("toml.lock"))?;
        let mut local = Self::load()?;
        update(local.owner_saves_mut(id)?)?;
        local.validate()?;
        save_atomic_locked(
            &local,
            &path,
            &local.revision,
            "Device-specific game installations",
            false,
        )
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        save_atomic(
            self,
            &installations_config_path()?,
            &self.revision,
            "Device-specific game installations",
            false,
        )
    }
}

// Import macro from crate root
use crate::documented_config;

// Implement DocumentedConfig trait for InstantGameConfig using the macro
documented_config!(
    InstantGameConfig,
    repo,
    "Path to restic backup repository",
    repo_password,
    "Password for restic repository",
    secret,
    games,
    "List of tracked games",
    example,
    r#"
[[games]]
name = "Example Game"
description = "Optional description"

[[games.dependencies]]
id = "example-save-data"
source_path = "~/Games/Example Game/Saves"
source_type = "directory"
"#,
    save_groups,
    "Shared save group identities and display names",
    retention_policy,
    "Optional top-level backup retention policy; omit to use defaults (keep-daily, keep-weekly, etc.)",
    example,
    r#"
[retention_policy]
keep_daily = 90
keep_weekly = 52
keep_monthly = 36
keep_yearly = 10
"#,
);

// Implement DocumentedConfig trait for InstallationsConfig using the macro
documented_config!(
    InstallationsConfig,
    installations,
    "List of game installations on this device",
    example,
    r#"
[[installations]]
game_name = "Example Game"
save_path = "~/Games/Example Game/Saves"
save_path_type = "directory"
"#,
    save_groups,
    "Device-specific shared save paths and synchronization state",
);

fn config_revision(path: &std::path::Path) -> Result<Option<String>> {
    use sha2::{Digest, Sha256};
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(hex::encode(Sha256::digest(bytes)))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Reading configuration revision"),
    }
}

fn save_atomic<T: DocumentedConfig>(
    config: &T,
    path: &std::path::Path,
    revision: &std::cell::RefCell<Option<String>>,
    header: &str,
    secret: bool,
) -> Result<()> {
    let parent = path
        .parent()
        .context("Configuration has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    let _lock = super::save_runtime::SaveLock::at_wait(&path.with_extension("toml.lock"))?;
    save_atomic_locked(config, path, revision, header, secret)
}

fn save_atomic_locked<T: DocumentedConfig>(
    config: &T,
    path: &std::path::Path,
    revision: &std::cell::RefCell<Option<String>>,
    header: &str,
    secret: bool,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path
        .parent()
        .context("Configuration has no parent directory")?;
    anyhow::ensure!(
        config_revision(path)? == *revision.borrow(),
        "Configuration {} changed since it was loaded; reload and retry (no changes were overwritten)",
        path.display()
    );
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    config.save_documented_pretty_toml(temporary.path(), Some(header))?;
    let mode = if secret {
        0o600
    } else {
        std::fs::metadata(path)
            .map(|m| m.permissions().mode())
            .unwrap_or(0o600)
    };
    temporary
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(mode))?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .context("Atomically replacing configuration")?;
    std::fs::File::open(parent)?.sync_all()?;
    *revision.borrow_mut() = config_revision(path)?;
    Ok(())
}

/// Check both revisions before writing either half of portable/local ownership.
/// A crash between replacements leaves an invalid or unbound mapping, which
/// validation blocks; it can never silently select another owner's save history.
pub(crate) fn save_pair(config: &InstantGameConfig, local: &InstallationsConfig) -> Result<()> {
    config.validate_ownership(local)?;
    let portable_path = games_config_path()?;
    let local_path = installations_config_path()?;
    std::fs::create_dir_all(
        portable_path
            .parent()
            .context("Configuration has no parent")?,
    )?;
    let _portable_lock =
        super::save_runtime::SaveLock::at_wait(&portable_path.with_extension("toml.lock"))?;
    let _local_lock =
        super::save_runtime::SaveLock::at_wait(&local_path.with_extension("toml.lock"))?;
    anyhow::ensure!(
        config_revision(&portable_path)? == *config.revision.borrow()
            && config_revision(&local_path)? == *local.revision.borrow(),
        "Game configuration changed since it was loaded; reload and retry (no changes were overwritten)"
    );
    save_atomic_locked(
        config,
        &portable_path,
        &config.revision,
        "Instant game configuration",
        true,
    )?;
    save_atomic_locked(
        local,
        &local_path,
        &local.revision,
        "Device-specific game installations",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn installation_sync_state_is_backwards_compatible() {
        let legacy = "game_name = 'Game'\nsave_path = '/saves'\nnearest_checkpoint = 'old'";
        let mut installation: GameInstallation = toml::from_str(legacy).unwrap();
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .sync_repository
                .is_none()
        );
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .acknowledged_snapshot
                .is_none()
        );
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .pending_restore
                .is_none()
        );
        let serialized = toml::to_string(&installation).unwrap();
        for field in [
            "sync_repository",
            "acknowledged_snapshot",
            "pending_restore",
        ] {
            assert!(!serialized.contains(field));
        }

        installation.saves.as_mut().unwrap().sync_repository = Some("repo".into());
        installation.saves.as_mut().unwrap().acknowledged_snapshot = Some("head".into());
        installation.saves.as_mut().unwrap().pending_restore = Some("historical".into());
        let restored: GameInstallation =
            toml::from_str(&toml::to_string(&installation).unwrap()).unwrap();
        assert_eq!(
            restored.owned().unwrap().sync_repository.as_deref(),
            Some("repo")
        );
        assert_eq!(
            restored.owned().unwrap().acknowledged_snapshot.as_deref(),
            Some("head")
        );
        assert_eq!(
            restored.owned().unwrap().pending_restore.as_deref(),
            Some("historical")
        );
        assert_eq!(
            restored.owned().unwrap().nearest_checkpoint.as_deref(),
            Some("old")
        );
    }

    #[test]
    fn repository_reconciliation_requires_a_known_different_repository() {
        let mut installation = GameInstallation::with_kind(
            "Game",
            TildePath::new(PathBuf::from("/saves")),
            PathContentKind::Directory,
        );
        assert!(
            !installation
                .owned()
                .unwrap()
                .needs_repository_reconciliation("repo")
        );
        installation.saves.as_mut().unwrap().sync_repository = Some("repo".into());
        assert!(
            !installation
                .owned()
                .unwrap()
                .needs_repository_reconciliation("repo")
        );
        assert!(
            installation
                .owned()
                .unwrap()
                .needs_repository_reconciliation("other")
        );
    }

    #[test]
    fn checkpoint_updates_clear_transient_sync_state() {
        let mut installation = GameInstallation::with_kind(
            "Game",
            TildePath::new(PathBuf::from("/saves")),
            PathContentKind::Directory,
        );
        installation.saves.as_mut().unwrap().sync_repository = Some("repo".into());

        // A fresh backup checkpoint clears transient sync state.
        installation.saves.as_mut().unwrap().acknowledged_snapshot = Some("head".into());
        installation.saves.as_mut().unwrap().pending_restore = Some("old".into());
        installation.owned_mut().unwrap().note_backup("old");
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .nearest_checkpoint
                .as_deref(),
            Some("old")
        );
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .checkpoint_time
                .is_some()
        );
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .acknowledged_snapshot
                .is_none()
        );
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .pending_restore
                .is_none()
        );
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .sync_repository
                .as_deref(),
            Some("repo")
        );

        // A completed restore clears it too, keeping only the acknowledged
        // head that belongs to the new state.
        installation.saves.as_mut().unwrap().acknowledged_snapshot = Some("head".into());
        installation.saves.as_mut().unwrap().pending_restore = Some("old".into());
        installation.owned_mut().unwrap().note_restored_snapshot(
            "old",
            "2026-01-01T00:00:00Z",
            Some("remote".into()),
        );
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .nearest_checkpoint
                .as_deref(),
            Some("old")
        );
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .checkpoint_time
                .as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .acknowledged_snapshot
                .as_deref(),
            Some("remote")
        );
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .pending_restore
                .is_none()
        );
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .sync_repository
                .as_deref(),
            Some("repo")
        );
    }

    #[test]
    fn restore_completion_persists_checkpoint_and_acknowledgement_together() {
        let mut installation = GameInstallation::with_kind(
            "Game",
            TildePath::new(PathBuf::from("/saves")),
            PathContentKind::Directory,
        );
        installation.saves.as_mut().unwrap().pending_restore = Some("historical".into());
        installation.owned_mut().unwrap().note_restored_snapshot(
            "historical",
            "2026-01-01T00:00:00Z",
            Some("remote-head".into()),
        );
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .nearest_checkpoint
                .as_deref(),
            Some("historical")
        );
        assert_eq!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .acknowledged_snapshot
                .as_deref(),
            Some("remote-head")
        );
        assert!(
            installation
                .saves
                .as_ref()
                .unwrap()
                .pending_restore
                .is_none()
        );
    }

    #[test]
    #[should_panic(expected = "acknowledged head must differ")]
    fn acknowledged_head_must_differ_from_the_restored_checkpoint() {
        let mut installation = GameInstallation::with_kind(
            "Game",
            TildePath::new(PathBuf::from("/saves")),
            PathContentKind::Directory,
        );
        installation.owned_mut().unwrap().note_restored_snapshot(
            "historical",
            "2026-01-01T00:00:00Z",
            Some("historical".into()),
        );
    }

    #[test]
    fn note_backup_updates_checkpoint() {
        let mut installation = GameInstallation::with_kind(
            GameName("test_game".to_string()),
            TildePath::new(PathBuf::from("~/.test/saves")),
            PathContentKind::Directory,
        );

        installation
            .owned_mut()
            .unwrap()
            .note_backup("checkpoint456");
        assert_eq!(
            installation.saves.as_ref().unwrap().nearest_checkpoint,
            Some("checkpoint456".to_string())
        );
        assert_eq!(installation.launch_command, None);
        assert!(installation.dependencies.is_empty());

        installation
            .owned_mut()
            .unwrap()
            .note_backup("checkpoint789");
        assert_eq!(
            installation.saves.as_ref().unwrap().nearest_checkpoint,
            Some("checkpoint789".to_string())
        );
        assert_eq!(installation.launch_command, None);
    }

    fn snapshot_fixture(id: &str, short_id: &str) -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "time": "2026-01-01T00:00:00Z",
            "tree": "0".repeat(64),
            "paths": [],
            "hostname": "test-host",
            "username": "test-user",
            "tags": [],
            "id": id,
            "short_id": short_id,
        }))
        .expect("snapshot fixture should deserialize")
    }

    #[test]
    fn test_checkpoint_matches_accepts_full_and_short_ids() {
        const FULL_ID: &str = "40dc152cb64e26ec6caa82e18a54e57e7ef5b8c9d1a4c7b16efc53a92b8a1234";
        const SHORT_ID: &str = "40dc152c";

        let mut installation = GameInstallation::with_kind(
            GameName("test_game".to_string()),
            TildePath::new(PathBuf::from("~/.test/saves")),
            PathContentKind::Directory,
        );
        let snapshot = snapshot_fixture(FULL_ID, SHORT_ID);

        // No checkpoint yet: never matches, sync must not skip.
        assert!(!installation.owned().unwrap().checkpoint_matches(&snapshot));

        // Writes are normalized to the full ID (see checkpoint.rs).
        installation.owned_mut().unwrap().note_backup(FULL_ID);
        assert!(installation.owned().unwrap().checkpoint_matches(&snapshot));

        // Legacy configs holding the short form (hand-edited or pre-
        // normalization) must still match, or sync re-restores over saves.
        installation.saves.as_mut().unwrap().nearest_checkpoint = Some(SHORT_ID.into());
        assert!(installation.owned().unwrap().checkpoint_matches(&snapshot));

        // Unrelated snapshots never match.
        let other = snapshot_fixture("deadbeef", "deadbeef");
        assert!(!installation.owned().unwrap().checkpoint_matches(&other));
    }

    #[test]
    fn test_retention_policy_defaults() {
        let policy = RetentionPolicyConfig::default();
        let effective = policy.effective();

        assert!(effective.keep_last.is_none());
        assert_eq!(
            effective.keep_daily,
            RetentionPolicyConfig::DEFAULT_KEEP_DAILY
        );
        assert_eq!(
            effective.keep_weekly,
            RetentionPolicyConfig::DEFAULT_KEEP_WEEKLY
        );
        assert_eq!(
            effective.keep_monthly,
            RetentionPolicyConfig::DEFAULT_KEEP_MONTHLY
        );
        assert_eq!(
            effective.keep_yearly,
            RetentionPolicyConfig::DEFAULT_KEEP_YEARLY
        );

        let rules = effective.to_rules();
        assert_eq!(rules.len(), 4);
        assert!(rules.contains(&("keep-daily".to_string(), "90".to_string())));
        assert!(rules.contains(&("keep-yearly".to_string(), "10".to_string())));
    }

    #[test]
    fn test_retention_policy_overrides() {
        let policy = RetentionPolicyConfig {
            keep_last: Some(5),
            keep_daily: Some(7),
            keep_weekly: Some(9),
            keep_monthly: Some(11),
            keep_yearly: Some(13),
        };

        let effective = policy.effective();

        assert_eq!(effective.keep_last, Some(5));
        assert_eq!(effective.keep_daily, 7);
        assert_eq!(effective.keep_weekly, 9);
        assert_eq!(effective.keep_monthly, 11);
        assert_eq!(effective.keep_yearly, 13);

        let rules = effective.to_rules();
        assert_eq!(rules.len(), 5);
        assert!(rules.contains(&("keep-last".to_string(), "5".to_string())));
        assert!(rules.contains(&("keep-yearly".to_string(), "13".to_string())));
    }

    #[test]
    fn test_validate_duplicate_games() {
        let toml_content = r#"
            repo = "~/.test/repo"
            repo_password = "pass"
            
            [[games]]
            name = "Game1"
            
            [[games]]
            name = "Game1"
        "#;

        let config: InstantGameConfig = toml::from_str(toml_content).expect("Parsing failed");
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate game name")
        );
    }

    #[test]
    fn test_validate_duplicate_installations() {
        let toml_content = r#"
            [[installations]]
            game_name = "Game1"
            save_path = "~/.saves/game1"
            
            [[installations]]
            game_name = "Game1"
            save_path = "~/.saves/game1_other"
        "#;

        let config: InstallationsConfig = toml::from_str(toml_content).expect("Parsing failed");
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate installation for game found")
        );
    }

    #[test]
    fn test_save_installations_uses_pretty_table_arrays() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("installations.toml");
        let config = InstallationsConfig {
            installations: vec![GameInstallation::with_kind(
                "Game1",
                TildePath::new(PathBuf::from("~/.saves/game1")),
                PathContentKind::Directory,
            )],
            ..Default::default()
        };

        config
            .save_documented_pretty_toml(&path, Some("Device-specific game installations"))
            .unwrap();

        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("# Available fields:"));
        assert!(
            contents.contains("# installations = []  # List of game installations on this device")
        );
        assert!(contents.contains("# Example installations entry:"));
        assert!(contents.contains("# [[installations]]"));
        assert!(contents.contains("# game_name = \"Example Game\""));
        assert!(contents.contains("# save_path = \"~/Games/Example Game/Saves\""));
        assert!(contents.contains("[[installations]]"));
        assert!(contents.contains("game_name = \"Game1\""));
        assert!(!contents.contains("installations = [{"));
    }

    #[test]
    fn test_save_games_uses_pretty_table_arrays() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("games.toml");
        let mut config = InstantGameConfig::default();
        config.games.push(Game::new("Game1"));

        config
            .save_documented_pretty_toml(&path, Some("Instant game configuration"))
            .unwrap();

        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains("# Available fields:"));
        assert!(contents.contains("# games = []  # List of tracked games"));
        assert!(contents.contains("# Example games entry:"));
        assert!(contents.contains("# [[games]]"));
        assert!(contents.contains("# name = \"Example Game\""));
        assert!(contents.contains("# [[games.dependencies]]"));
        assert!(contents.contains("# retention_policy"));
        assert!(contents.contains("Optional top-level backup retention policy"));
        assert!(contents.contains("# Example retention_policy entry:"));
        assert!(contents.contains("# [retention_policy]"));
        assert!(contents.contains("# keep_daily = 90"));
        assert!(contents.contains("[[games]]"));
        assert!(contents.contains("name = \"Game1\""));
        assert!(!contents.contains("games = [{"));
    }

    #[test]
    fn test_save_games_documents_repo_password_without_default() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("games.toml");
        let config = InstantGameConfig::default();

        config
            .save_documented_pretty_toml(&path, Some("Instant game configuration"))
            .unwrap();

        let contents = fs::read_to_string(path).unwrap();
        // Secret fields should still be discoverable, but their default value
        // must not be advertised in the documentation block.
        let docs_end = contents.find("\n\n").unwrap_or(contents.len());
        let docs_block = &contents[..docs_end];
        assert!(
            docs_block.contains("# repo_password  # Password for restic repository"),
            "repo_password missing from docs header: {docs_block}"
        );
        assert!(
            !docs_block.contains("instantgamepassword"),
            "default password leaked into docs header: {docs_block}"
        );
        // The current value still has to round-trip via the TOML body.
        assert!(contents.contains("repo_password = "));
    }

    #[test]
    fn stale_config_writes_do_not_erase_newer_bindings() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("installations.toml");
        let mut first = InstallationsConfig::default();
        save_atomic(&first, &path, &first.revision, "test", false)?;
        let mut second = first.clone();
        second.installations.push(GameInstallation::with_kind(
            "B",
            TildePath::from_str("/b"),
            PathContentKind::Directory,
        ));
        save_atomic(&second, &path, &second.revision, "test", false)?;
        first.installations.push(GameInstallation::with_kind(
            "A",
            TildePath::from_str("/a"),
            PathContentKind::Directory,
        ));
        assert!(save_atomic(&first, &path, &first.revision, "test", false).is_err());
        let saved: InstallationsConfig = toml::from_str(&std::fs::read_to_string(&path)?)?;
        assert_eq!(saved.installations.len(), 1);
        assert_eq!(saved.installations[0].game_name.0, "B");
        Ok(())
    }

    #[test]
    fn test_repo_password_marked_secret_in_metadata() {
        let metadata = InstantGameConfig::field_metadata();
        let repo_password = metadata.iter().find(|f| f.name == "repo_password").unwrap();
        assert!(
            repo_password.secret,
            "repo_password should be marked secret so its default is not written into docs"
        );

        // Sanity: a non-secret field on the same config is not flagged.
        let games = metadata.iter().find(|f| f.name == "games").unwrap();
        assert!(!games.secret);
    }
}
