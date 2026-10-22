use anyhow::{Context, Result, ensure};

use super::cli::SaveGroupCommands;
use super::config::{
    InstallationsConfig, InstantGameConfig, OwnedSaves, PathContentKind, SaveGroupBinding,
    SharedSaveGroup,
};
use super::ownership::{OwnerId, SaveOwner};
use super::reconciliation::SaveChoice;
use super::save_runtime::{self, SaveLock};
use crate::common::TildePath;

pub fn handle(command: SaveGroupCommands) -> Result<()> {
    let mut config = InstantGameConfig::load()?;
    let mut local = InstallationsConfig::load()?;
    match command {
        SaveGroupCommands::Create {
            id,
            name,
            path,
            file,
        } => {
            let _lock = SaveLock::owner(&OwnerId::Group(id.clone()))?;
            ensure!(
                !config.save_groups.iter().any(|group| group.id == id),
                "Group '{id}' already exists"
            );
            config.save_groups.push(SharedSaveGroup {
                id: id.clone(),
                name,
            });
            config.validate()?;
            if let Some(path) = path {
                bind(&config, &mut local, &id, &path, file)?;
            }
            super::config::save_pair(&config, &local)?;
        }
        SaveGroupCommands::Migrate { id, name, games } => {
            // Match join's lock order, with stable ordering among private owners.
            let mut owners: Vec<_> = games.iter().cloned().map(OwnerId::Game).collect();
            owners.push(OwnerId::Group(id.clone()));
            owners.sort();
            owners.dedup();
            let _locks = owners
                .iter()
                .map(SaveLock::owner)
                .collect::<Result<Vec<_>>>()?;
            config = InstantGameConfig::load()?;
            local = InstallationsConfig::load()?;
            migrate_bindings(&mut config, &mut local, &id, &name, &games)?;
            super::config::save_pair(&config, &local)?;
            println!(
                "Migrated {} games to '{id}'. Save files and private snapshot histories were retained. Use group reconcile {id} --local to publish current saves, or group import to select an old private snapshot.",
                games.len()
            );
        }
        SaveGroupCommands::Bind { id, path, file } => {
            let _lock = SaveLock::owner(&OwnerId::Group(id.clone()))?;
            bind(&config, &mut local, &id, &path, file)?;
            local.save()?;
        }
        SaveGroupCommands::Join {
            id,
            game,
            discard_private_binding,
        } => {
            let _game_lock = SaveLock::owner(&OwnerId::Game(game.clone()))?;
            let _group_lock = SaveLock::owner(&OwnerId::Group(id.clone()))?;
            ensure!(
                config.save_groups.iter().any(|group| group.id == id),
                "Unknown group '{id}'"
            );
            let index = config
                .games
                .iter()
                .position(|entry| entry.name.0 == game)
                .context("Game is not configured")?;
            if let Some(previous) = &config.games[index].save_group {
                ensure!(
                    previous == &id,
                    "Game already belongs to group '{previous}'; change membership explicitly in games.toml after stopping play"
                );
            }
            if let Some(installation) = local
                .installations
                .iter_mut()
                .find(|entry| entry.game_name.0 == game)
            {
                ensure!(
                    installation.saves.is_none() || discard_private_binding,
                    "Game has private saves. Choose a group baseline, then repeat with --discard-private-binding to remove its old path and sync state. Snapshots and save files will be retained"
                );
                ensure!(
                    installation
                        .saves
                        .as_ref()
                        .is_none_or(|saves| saves.pending_restore.is_none()),
                    "Finish the incomplete restore for '{game}' before joining a save group"
                );
                installation.saves = None;
            }
            config.games[index].save_group = Some(id);
            config.validate_ownership(&local)?;
            // Mixed private/shared ownership blocks mutations if a crash occurs
            // between the portable and local configuration replacements.
            super::config::save_pair(&config, &local)?;
        }
        SaveGroupCommands::Remove { id } => {
            let _lock = SaveLock::owner(&OwnerId::Group(id.clone()))?;
            ensure!(
                !config
                    .games
                    .iter()
                    .any(|game| game.save_group.as_deref() == Some(&id)),
                "Group '{id}' still has member games"
            );
            ensure!(
                config.save_groups.iter().any(|group| group.id == id),
                "Unknown group '{id}'"
            );
            local.save_groups.retain(|binding| binding.id != id);
            local.save()?;
            config.save_groups.retain(|group| group.id != id);
            config.save()?;
        }
        SaveGroupCommands::List => {
            for group in &config.save_groups {
                let binding = local
                    .save_groups
                    .iter()
                    .find(|binding| binding.id == group.id);
                println!(
                    "{}: {} — {}",
                    group.id,
                    group.name,
                    binding
                        .map(|binding| binding.saves.save_path.display_string())
                        .unwrap_or_else(|| "not bound on this device".into())
                );
                let members: Vec<_> = config
                    .games
                    .iter()
                    .filter(|game| game.save_group.as_deref() == Some(&group.id))
                    .map(|game| game.name.0.as_str())
                    .collect();
                println!("  Games: {}", members.join(", "));
            }
        }
        SaveGroupCommands::Reconcile {
            id,
            local,
            snapshot,
        } => return reconcile(&config, &OwnerId::Group(id), local, snapshot),
        SaveGroupCommands::Restore {
            id,
            snapshot,
            force,
        } => return super::restic::restore_owner(&config, &OwnerId::Group(id), snapshot, force),
        SaveGroupCommands::Sync { id } => {
            let id = OwnerId::Group(id);
            let _lock = SaveLock::owner(&id)?;
            config.validate_ownership(&local)?;
            let owner = config.resolve_owner(&local, &id)?;
            super::operations::sync::sync_single_owner(&owner, &config)?;
        }
        SaveGroupCommands::Snapshots { id } => {
            ensure!(
                config.save_groups.iter().any(|group| group.id == id),
                "Unknown group '{id}'"
            );
            for snapshot in save_runtime::fresh_snapshots(&config, &OwnerId::Group(id))? {
                println!("{} {} {}", snapshot.id, snapshot.time, snapshot.hostname);
            }
        }
        SaveGroupCommands::Import { id, game, snapshot } => {
            return import_snapshot(&config, &local, &id, &game, &snapshot);
        }
    }
    Ok(())
}

/// Prepare the complete conversion before validating ownership. This deliberately
/// accepts duplicate input paths, but never leaves a partially converted model.
fn migrate_bindings(
    config: &mut InstantGameConfig,
    local: &mut InstallationsConfig,
    id: &str,
    name: &str,
    games: &[String],
) -> Result<()> {
    let selected: std::collections::HashSet<_> = games.iter().collect();
    ensure!(
        selected.len() == games.len() && selected.len() >= 2,
        "Migration requires at least two distinct private games"
    );
    ensure!(
        !config.save_groups.iter().any(|group| group.id == id),
        "Group '{id}' already exists; migrate to a new group ID"
    );
    let mut next_config = config.clone();
    let mut next_local = local.clone();
    let mut binding: Option<OwnedSaves> = None;
    for name in games {
        let game = next_config
            .games
            .iter_mut()
            .find(|game| game.name.0 == *name)
            .with_context(|| format!("Game '{name}' is not configured"))?;
        ensure!(
            game.save_group.is_none(),
            "Game '{name}' already belongs to a group"
        );
        let installation = next_local
            .installations
            .iter_mut()
            .find(|entry| entry.game_name.0 == *name)
            .with_context(|| format!("Game '{name}' has no local private binding"))?;
        let saves = installation.owned()?;
        ensure!(
            saves.pending_restore.is_none(),
            "Finish the incomplete restore for '{name}' before migrating saves"
        );
        if let Some(first) = &binding {
            ensure!(
                first.save_path_type == saves.save_path_type
                    && super::ownership::normalized_path(first.save_path.as_path())?
                        == super::ownership::normalized_path(saves.save_path.as_path())?,
                "Migration only combines games using exactly the same save path and type; independent cards must retain separate histories"
            );
        } else {
            // Private checkpoints belong to private histories. Start the group
            // without claiming that any group snapshot has been synchronized.
            binding = Some(OwnedSaves::new(
                saves.save_path.clone(),
                saves.save_path_type,
            ));
        }
        installation.saves = None;
        game.save_group = Some(id.into());
    }
    next_config.save_groups.push(SharedSaveGroup {
        id: id.into(),
        name: name.into(),
    });
    next_local.save_groups.push(SaveGroupBinding {
        id: id.into(),
        saves: binding.context("No private saves selected")?,
    });
    next_config.validate_ownership(&next_local)?;
    *config = next_config;
    *local = next_local;
    Ok(())
}

fn bind(
    config: &InstantGameConfig,
    local: &mut InstallationsConfig,
    id: &str,
    path: &str,
    file: bool,
) -> Result<()> {
    ensure!(
        config.save_groups.iter().any(|group| group.id == id),
        "Unknown group '{id}'"
    );
    let path = TildePath::from_str(path);
    let kind = match std::fs::metadata(path.as_path()) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file() || metadata.is_dir(),
                "Save path must be a file or directory"
            );
            ensure!(
                !file || metadata.is_file(),
                "--file requires a file save path"
            );
            PathContentKind::from(metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if file {
                PathContentKind::File
            } else {
                PathContentKind::Directory
            }
        }
        Err(error) => return Err(error).context("Reading group save path"),
    };
    crate::game::utils::safeguards::ensure_safe_path(
        path.as_path(),
        crate::game::utils::safeguards::PathUsage::SaveDirectory,
    )?;
    let saves = OwnedSaves::new(path, kind);
    if let Some(binding) = local
        .save_groups
        .iter_mut()
        .find(|binding| binding.id == id)
    {
        ensure!(
            binding.saves.pending_restore.is_none(),
            "Finish the incomplete restore before changing this group's path"
        );
        binding.saves = saves;
    } else {
        local.save_groups.push(SaveGroupBinding {
            id: id.into(),
            saves,
        });
    }
    config.validate_ownership(local)
}

pub fn reconcile(
    config: &InstantGameConfig,
    id: &OwnerId,
    accept_local: bool,
    snapshot: Option<String>,
) -> Result<()> {
    if !accept_local && snapshot.is_none() {
        super::reconciliation::reconcile_owner(config, id)?;
        return Ok(());
    }
    let _lock = SaveLock::owner(id)?;
    let local = InstallationsConfig::load()?;
    config.validate_ownership(&local)?;
    let owner = config.resolve_owner(&local, id)?;
    println!("{}", owner.scope());
    let choice = match snapshot {
        Some(snapshot) => SaveChoice::Restore(snapshot),
        None => SaveChoice::Upload,
    };
    super::reconciliation::apply_owner_choice(config, &owner, choice)
}

fn import_snapshot(
    config: &InstantGameConfig,
    local: &InstallationsConfig,
    id: &str,
    game: &str,
    snapshot: &str,
) -> Result<()> {
    let id = OwnerId::Group(id.into());
    let _lock = SaveLock::owner(&id)?;
    config.validate_ownership(local)?;
    let owner = config.resolve_owner(local, &id)?;
    let source_id = OwnerId::Game(game.into());
    let source = super::restic::cache::get_snapshot_for_owner(snapshot, &source_id, config)?
        .context("Snapshot does not belong to the selected private game history")?;
    ensure!(
        source.paths.len() == 1,
        "Import requires exactly one source file or directory; convert this layout explicitly"
    );
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("imported-saves");
    let handler = super::restic::backup::GameBackup::new(config.clone());
    handler.restore_untracked(&super::restic::backup::RestoreRequest {
        owner: &source_id,
        snapshot_id: &source.id,
        snapshot_time: &source.time,
        path: &path,
        save_path_type: owner.save_path_type,
        snapshot_source_path: source.paths.first().map(String::as_str),
        observed_head: None,
    })?;
    let parents = save_runtime::history_heads(&save_runtime::fresh_snapshots(config, &id)?)?;
    let imported = SaveOwner {
        saves: OwnedSaves::new(TildePath::new(path), owner.save_path_type),
        ..owner.clone()
    };
    let backup = handler.backup_owner(&imported, &save_runtime::parent_tags(&parents))?;
    let snapshot = backup
        .snapshot_id
        .context("Import did not create a snapshot; original private snapshot is retained")?;
    super::operations::sync::execution::perform_restore(&owner, config, &snapshot)
}

pub fn add_member(
    group: &str,
    name: &str,
    description: Option<String>,
    launch_command: Option<String>,
) -> Result<()> {
    let id = OwnerId::Group(group.into());
    let _lock = SaveLock::owner(&id)?;
    let mut config = InstantGameConfig::load()?;
    ensure!(
        config.save_groups.iter().any(|entry| entry.id == group),
        "Unknown save group '{group}'"
    );
    ensure!(!name.trim().is_empty(), "Game name cannot be empty");
    ensure!(
        !config.games.iter().any(|entry| entry.name.0 == name),
        "Game '{name}' already exists; use group join to change its ownership explicitly"
    );
    let mut game = super::config::Game::new(name);
    game.description = description;
    game.launch_command =
        launch_command.map(super::launch_command::LaunchCommand::from_shell_or_manual);
    game.save_group = Some(group.into());
    config.games.push(game);
    config.validate_ownership(&InstallationsConfig::load()?)?;
    config.save()?;
    println!("Added '{name}' using shared save group '{group}'");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::config::{Game, GameInstallation};

    fn private_pair(path: &std::path::Path) -> (InstantGameConfig, InstallationsConfig) {
        (
            InstantGameConfig {
                games: vec![Game::new("A"), Game::new("B")],
                ..Default::default()
            },
            InstallationsConfig {
                installations: ["A", "B"]
                    .into_iter()
                    .map(|name| {
                        let mut installation = GameInstallation::with_kind(
                            name,
                            TildePath::new(path.to_path_buf()),
                            PathContentKind::File,
                        );
                        installation.saves.as_mut().unwrap().nearest_checkpoint =
                            Some(format!("{name}-private-head"));
                        installation
                    })
                    .collect(),
                ..Default::default()
            },
        )
    }

    #[test]
    fn converts_duplicate_bindings_together_and_resets_private_history_state() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("shared.ps2");
        std::fs::write(&path, "progress")?;
        let (mut config, mut local) = private_pair(&path);
        assert!(config.validate_ownership(&local).is_err());
        migrate_bindings(
            &mut config,
            &mut local,
            "card",
            "Shared card",
            &["A".into(), "B".into()],
        )?;
        config.validate_ownership(&local)?;
        assert!(
            config
                .games
                .iter()
                .all(|game| game.save_group.as_deref() == Some("card"))
        );
        assert!(local.installations.iter().all(|game| game.saves.is_none()));
        let saves = &local.save_groups[0].saves;
        assert_eq!(saves.save_path.as_path(), path);
        assert!(saves.nearest_checkpoint.is_none());
        assert!(saves.baseline.is_none());
        assert_eq!(std::fs::read_to_string(path)?, "progress");
        Ok(())
    }

    #[test]
    fn rejects_unfinished_restores_and_independent_cards_without_partial_conversion() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("shared.ps2");
        let (config, local) = private_pair(&path);
        for pending in [true, false] {
            let mut config = config.clone();
            let mut local = local.clone();
            let second = local.installations[1].saves.as_mut().unwrap();
            if pending {
                second.pending_restore = Some("interrupted-snapshot".into());
            } else {
                second.save_path = TildePath::new(temp.path().join("independent.ps2"));
            }
            let before_config = toml::to_string(&config)?;
            let before_local = toml::to_string(&local)?;
            let error = migrate_bindings(
                &mut config,
                &mut local,
                "card",
                "Shared card",
                &["A".into(), "B".into()],
            )
            .unwrap_err();
            assert!(error.to_string().contains(if pending {
                "incomplete restore"
            } else {
                "exactly the same save path"
            }));
            assert_eq!(toml::to_string(&config)?, before_config);
            assert_eq!(toml::to_string(&local)?, before_local);
        }
        Ok(())
    }

    #[test]
    fn rejects_incomplete_selection_of_duplicate_owners() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("shared.ps2");
        let (mut config, mut local) = private_pair(&path);
        config.games.push(Game::new("C"));
        local.installations.push(GameInstallation::with_kind(
            "C",
            TildePath::new(path),
            PathContentKind::File,
        ));
        assert!(
            migrate_bindings(
                &mut config,
                &mut local,
                "card",
                "Shared card",
                &["A".into(), "B".into()]
            )
            .is_err()
        );
        assert!(config.save_groups.is_empty());
        assert!(
            local
                .installations
                .iter()
                .all(|entry| entry.saves.is_some())
        );
        Ok(())
    }
}
