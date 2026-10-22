//! Resolve portable membership and local paths before touching save data.
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use super::config::{InstallationsConfig, InstantGameConfig, OwnedSaves};
use super::restic::tags;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OwnerId {
    Game(String),
    Group(String),
}

impl OwnerId {
    pub fn tags(&self) -> Vec<String> {
        match self {
            Self::Game(name) => tags::create_game_tags(name),
            Self::Group(id) => vec![
                "instantgame-group".into(),
                tags::encode_game_name_for_tag(id),
            ],
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Game(name) => name.clone(),
            Self::Group(id) => format!("save group '{id}'"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SaveOwner {
    pub id: OwnerId,
    pub name: String,
    pub members: Vec<String>,
    pub saves: OwnedSaves,
}

impl std::ops::Deref for SaveOwner {
    type Target = OwnedSaves;
    fn deref(&self) -> &Self::Target {
        &self.saves
    }
}

impl SaveOwner {
    pub fn scope(&self) -> String {
        match self.id {
            OwnerId::Game(_) => format!("{}: {}", self.name, self.save_path.display_string()),
            OwnerId::Group(_) => format!(
                "Shared save group: {}\nDestination: {}\nGames: {}\nAll contained saves roll back together when restored.",
                self.name,
                self.save_path.display_string(),
                self.members.join(", ")
            ),
        }
    }
}

impl InstantGameConfig {
    pub fn owner_id(&self, game: &str) -> OwnerId {
        match self
            .games
            .iter()
            .find(|entry| entry.name.0 == game)
            .and_then(|entry| entry.save_group.as_ref())
        {
            Some(group) => OwnerId::Group(group.clone()),
            None => OwnerId::Game(game.into()),
        }
    }

    pub fn validate_ownership(&self, local: &InstallationsConfig) -> Result<()> {
        self.validate()?;
        local.validate()?;
        let mut paths = Vec::new();
        for installation in &local.installations {
            match (
                self.owner_id(&installation.game_name.0),
                &installation.saves,
            ) {
                (OwnerId::Group(id), Some(_)) => bail!(
                    "Game '{}' belongs to save group '{id}' but still has private save fields. Choose a group baseline explicitly, then remove its private save fields; old snapshots are retained.",
                    installation.game_name
                ),
                (OwnerId::Game(_), None) => bail!(
                    "Private game '{}' requires a save_path",
                    installation.game_name
                ),
                (OwnerId::Game(name), Some(saves)) => paths.push((
                    OwnerId::Game(name),
                    normalized_path(saves.save_path.as_path())?,
                )),
                _ => {}
            }
        }
        for binding in &local.save_groups {
            ensure!(
                self.save_groups.iter().any(|group| group.id == binding.id),
                "Local save group '{}' is not declared in games.toml",
                binding.id
            );
            paths.push((
                OwnerId::Group(binding.id.clone()),
                normalized_path(binding.saves.save_path.as_path())?,
            ));
        }
        let config_path = normalized_path(&crate::common::paths::games_config_dir()?)?;
        for (owner, path) in &paths {
            super::utils::safeguards::ensure_safe_path(
                path,
                super::utils::safeguards::PathUsage::SaveDirectory,
            )?;
            ensure!(
                !config_path.starts_with(path),
                "{} contains InstantCLI's configuration; choose the save-data directory",
                owner.label()
            );
            let repository = self.repo.as_path();
            if !repository.as_os_str().is_empty()
                && (repository.is_absolute() || repository.exists())
            {
                let repository = normalized_path(repository)?;
                ensure!(
                    !repository.starts_with(path) && !path.starts_with(&repository),
                    "{} overlaps its backup repository",
                    owner.label()
                );
            }
        }
        for (index, (owner, path)) in paths.iter().enumerate() {
            for (other, other_path) in &paths[..index] {
                ensure!(
                    !path.starts_with(other_path) && !other_path.starts_with(path),
                    "Conflicting save ownership: {} ({}) overlaps {} ({})",
                    owner.label(),
                    path.display(),
                    other.label(),
                    other_path.display()
                );
                // canonicalize handles symlinks; dev/inode catches hard-linked files.
                #[cfg(unix)]
                if let (Ok(a), Ok(b)) = (std::fs::metadata(path), std::fs::metadata(other_path)) {
                    use std::os::unix::fs::MetadataExt;
                    ensure!(
                        a.dev() != b.dev() || a.ino() != b.ino(),
                        "Conflicting save ownership: {} and {} refer to the same inode",
                        owner.label(),
                        other.label()
                    );
                }
            }
        }
        Ok(())
    }

    pub fn resolve_owner(&self, local: &InstallationsConfig, id: &OwnerId) -> Result<SaveOwner> {
        let (name, members) = match id {
            OwnerId::Game(name) => (name.clone(), vec![name.clone()]),
            OwnerId::Group(id) => {
                let group = self
                    .save_groups
                    .iter()
                    .find(|group| &group.id == id)
                    .with_context(|| format!("Unknown save group '{id}'"))?;
                (
                    group.name.clone(),
                    self.games
                        .iter()
                        .filter(|game| game.save_group.as_ref() == Some(id))
                        .map(|game| game.name.0.clone())
                        .collect(),
                )
            }
        };
        let mut saves = local.owner_saves(id)?.clone();
        saves.save_path =
            crate::common::TildePath::new(normalized_path(saves.save_path.as_path())?);
        Ok(SaveOwner {
            id: id.clone(),
            name,
            members,
            saves,
        })
    }

    pub fn bound_owner_ids(&self, local: &InstallationsConfig) -> Vec<OwnerId> {
        let mut ids: Vec<_> = local
            .installations
            .iter()
            .filter(|entry| entry.saves.is_some())
            .map(|entry| OwnerId::Game(entry.game_name.0.clone()))
            .collect();
        ids.extend(
            local
                .save_groups
                .iter()
                .map(|group| OwnerId::Group(group.id.clone())),
        );
        ids.sort();
        ids.dedup();
        ids
    }
}

impl InstallationsConfig {
    pub fn owner_saves(&self, id: &OwnerId) -> Result<&OwnedSaves> {
        match id {
            OwnerId::Game(name) => self
                .installations
                .iter()
                .find(|entry| &entry.game_name.0 == name)
                .and_then(|entry| entry.saves.as_ref()),
            OwnerId::Group(id) => self
                .save_groups
                .iter()
                .find(|entry| &entry.id == id)
                .map(|entry| &entry.saves),
        }
        .with_context(|| {
            format!(
                "No local save binding for {}; configure installations.toml",
                id.label()
            )
        })
    }

    pub fn owner_saves_mut(&mut self, id: &OwnerId) -> Result<&mut OwnedSaves> {
        match id {
            OwnerId::Game(name) => self
                .installations
                .iter_mut()
                .find(|entry| &entry.game_name.0 == name)
                .and_then(|entry| entry.saves.as_mut()),
            OwnerId::Group(id) => self
                .save_groups
                .iter_mut()
                .find(|entry| &entry.id == id)
                .map(|entry| &mut entry.saves),
        }
        .with_context(|| format!("No local save binding for {}", id.label()))
    }
}

pub(super) fn validate_groups(config: &InstantGameConfig) -> Result<()> {
    let mut ids = HashSet::new();
    for group in &config.save_groups {
        ensure!(
            !group.id.is_empty()
                && group.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'
                    || b == b'_'),
            "Save group ID '{}' must be a nonempty lowercase slug",
            group.id
        );
        ensure!(
            !group.name.trim().is_empty(),
            "Save group '{}' requires a display name",
            group.id
        );
        ensure!(ids.insert(&group.id), "Duplicate save group '{}'", group.id);
    }
    for game in &config.games {
        if let Some(id) = &game.save_group {
            ensure!(
                ids.contains(id),
                "Game '{}' references undeclared save group '{id}'",
                game.name
            );
        }
    }
    Ok(())
}

/// Canonicalize the existing prefix, preserving missing suffixes. Resolve `..`
/// after symlinks rather than lexically erasing a potentially different parent.
pub fn normalized_path(path: &Path) -> Result<PathBuf> {
    ensure!(!path.as_os_str().is_empty(), "Save path cannot be empty");
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => {
                resolved.push(other.as_os_str());
                match std::fs::canonicalize(&resolved) {
                    Ok(path) => resolved = path,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        ensure!(
                            !resolved.is_symlink(),
                            "Dangling save-path symlink: {}",
                            resolved.display()
                        );
                    }
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("Resolving save path {}", resolved.display())
                        });
                    }
                }
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::game::config::{
        Game, GameInstallation, PathContentKind, SaveGroupBinding, SharedSaveGroup,
    };

    fn shared() -> (InstantGameConfig, InstallationsConfig) {
        let mut config = InstantGameConfig::default();
        config.save_groups.push(SharedSaveGroup {
            id: "ps2".into(),
            name: "PS2 cards".into(),
        });
        for name in ["A", "B"] {
            let mut game = Game::new(name);
            game.save_group = Some("ps2".into());
            config.games.push(game);
        }
        let mut local = InstallationsConfig::default();
        local.save_groups.push(SaveGroupBinding {
            id: "ps2".into(),
            saves: OwnedSaves::new(
                TildePath::from_str("/saves/ps2"),
                PathContentKind::Directory,
            ),
        });
        (config, local)
    }

    #[test]
    fn members_share_one_owner_without_installation_records() -> Result<()> {
        let (config, local) = shared();
        config.validate_ownership(&local)?;
        assert_eq!(config.owner_id("A"), config.owner_id("B"));
        assert_eq!(
            config.bound_owner_ids(&local),
            vec![OwnerId::Group("ps2".into())]
        );
        let owner = config.resolve_owner(&local, &config.owner_id("A"))?;
        assert_eq!(owner.members, ["A", "B"]);
        assert!(owner.scope().contains("All contained saves"));
        Ok(())
    }

    #[test]
    fn group_and_same_named_game_have_disjoint_history_tags() {
        let private = OwnerId::Game("ps2".into()).tags();
        let shared = OwnerId::Group("ps2".into()).tags();
        assert_ne!(private, shared);
        assert!(!private.iter().all(|tag| shared.contains(tag)));
    }

    #[test]
    fn rejects_private_state_on_shared_member_and_unknown_references() {
        let (mut config, mut local) = shared();
        local.installations.push(GameInstallation::with_kind(
            "A",
            TildePath::from_str("/private"),
            PathContentKind::Directory,
        ));
        assert!(
            config
                .validate_ownership(&local)
                .unwrap_err()
                .to_string()
                .contains("private save fields")
        );
        config.games[0].save_group = Some("missing".into());
        assert!(config.validate().is_err());
    }

    #[test]
    fn unbound_groups_do_not_fall_back_to_game_history() -> Result<()> {
        let (config, mut local) = shared();
        local.save_groups.clear();
        config.validate_ownership(&local)?;
        assert!(config.resolve_owner(&local, &config.owner_id("A")).is_err());
        Ok(())
    }

    #[test]
    fn rejects_nested_symlinked_missing_and_hard_linked_paths() -> Result<()> {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let tree = temp.path().join("tree");
        std::fs::create_dir(&tree)?;
        let alias = temp.path().join("alias");
        symlink(&tree, &alias)?;
        let config = InstantGameConfig::default();
        let mut local = InstallationsConfig::default();
        local.installations.push(GameInstallation::with_kind(
            "A",
            TildePath::new(tree.clone()),
            PathContentKind::Directory,
        ));
        local.installations.push(GameInstallation::with_kind(
            "B",
            TildePath::new(alias.join("missing/subdir")),
            PathContentKind::Directory,
        ));
        assert!(config.validate_ownership(&local).is_err());
        std::fs::write(tree.join("a"), b"card")?;
        std::fs::hard_link(tree.join("a"), temp.path().join("b"))?;
        local.installations[0] =
            GameInstallation::with_kind("A", TildePath::new(tree.join("a")), PathContentKind::File);
        local.installations[1] = GameInstallation::with_kind(
            "B",
            TildePath::new(temp.path().join("b")),
            PathContentKind::File,
        );
        assert!(config.validate_ownership(&local).is_err());
        Ok(())
    }

    #[test]
    fn shared_override_roundtrips_without_private_fields_and_rejects_partial_state() -> Result<()> {
        let installation: GameInstallation =
            toml::from_str("game_name = 'A'\nlaunch_command = 'true'\n")?;
        assert!(installation.saves.is_none());
        let encoded = toml::to_string_pretty(&installation)?;
        assert!(!encoded.contains("save_path"));
        assert!(
            toml::from_str::<GameInstallation>("game_name = 'A'\npending_restore = 'id'\n")
                .is_err()
        );
        assert!(
            toml::from_str::<GameInstallation>(
                "game_name = 'A'\nsave_path = '/saves'\nsave_path_type = 'typo'\n"
            )
            .is_err()
        );
        let group: SaveGroupBinding = toml::from_str("id = 'ps2'\nsave_path = '/saves'\n")?;
        let decoded: SaveGroupBinding = toml::from_str(&toml::to_string_pretty(&group)?)?;
        assert_eq!(decoded.id, "ps2");
        Ok(())
    }
}
