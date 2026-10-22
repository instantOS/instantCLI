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

pub fn configure_discovered(path: &str, display_name: &str) -> Result<()> {
    use crate::menu_utils::{DialogOutcome, FzfWrapper};
    let local = InstallationsConfig::load()?;
    let normalized = super::ownership::normalized_path(std::path::Path::new(path))?;
    let existing = local.save_groups.iter().find(|binding| {
        super::ownership::normalized_path(binding.saves.save_path.as_path())
            .is_ok_and(|bound| bound == normalized)
    });
    let id = if let Some(binding) = existing {
        binding.id.clone()
    } else {
        let id = match FzfWrapper::input("Name this shared save group (portable ID, e.g. ps2)")? {
            DialogOutcome::Submitted(value) if !value.trim().is_empty() => value.trim().to_owned(),
            _ => return Ok(()),
        };
        let config = InstantGameConfig::load()?;
        if config.save_groups.iter().any(|group| group.id == id) {
            handle(SaveGroupCommands::Bind {
                id: id.clone(),
                path: path.into(),
                file: false,
            })?;
        } else {
            handle(SaveGroupCommands::Create {
                id: id.clone(),
                name: display_name.into(),
                path: Some(path.into()),
                file: false,
            })?;
        }
        id
    };
    FzfWrapper::message(&format!(
        "Shared saves '{id}' are configured at {path}. All games in this group share one backup history; restoring rolls back every contained card together."
    ))?;
    let name =
        match FzfWrapper::input("Add a game using these shared saves (leave empty to finish)")? {
            DialogOutcome::Submitted(value) if !value.trim().is_empty() => value.trim().to_owned(),
            _ => return Ok(()),
        };
    add_member(&id, &name, None, None)?;
    super::menu::game_menu(Some(name))
}
