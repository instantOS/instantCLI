pub mod backup;
pub mod cache;
pub mod commands;
pub mod dependencies;
pub mod prune;
pub mod single_file;
pub mod snapshot_selection;
pub mod snapshots;
pub mod tags;

use crate::game::config::InstantGameConfig;
use anyhow::{Context, Result};

/// Manual backups use the same owner lock and state transitions as automatic sync.
pub fn backup_game_saves(game_name: Option<String>) -> Result<()> {
    let Some(name) = select_game_name(game_name)? else {
        return Ok(());
    };
    let config = InstantGameConfig::load()?;
    let id = config.owner_id(&name);
    let _lock = crate::game::save_runtime::SaveLock::owner(&id)?;
    let local = crate::game::config::InstallationsConfig::load()?;
    config.validate_ownership(&local)?;
    let owner = config.resolve_owner(&local, &id)?;
    crate::game::save_runtime::ensure_ready(&owner, &config)?;
    crate::game::operations::sync::execution::publish_local_choice(&owner, &config)
}

pub fn restore_game_saves(
    game_name: Option<String>,
    snapshot_id: Option<String>,
    force: bool,
) -> Result<()> {
    let Some(name) = select_game_name(game_name)? else {
        return Ok(());
    };
    let config = InstantGameConfig::load()?;
    restore_owner(&config, &config.owner_id(&name), snapshot_id, force)
}

pub(crate) fn restore_owner(
    config: &InstantGameConfig,
    id: &crate::game::ownership::OwnerId,
    snapshot_id: Option<String>,
    force: bool,
) -> Result<()> {
    let _lock = crate::game::save_runtime::SaveLock::owner(id)?;
    let local = crate::game::config::InstallationsConfig::load()?;
    config.validate_ownership(&local)?;
    let owner = config.resolve_owner(&local, id)?;
    let snapshots = crate::game::save_runtime::fresh_snapshots(config, id)?;
    let snapshot = match snapshot_id {
        Some(id) => snapshots
            .iter()
            .find(|snapshot| snapshot.matches_id(&id))
            .context("Snapshot does not belong to this save owner")?
            .clone(),
        None => {
            use crate::game::restic::snapshot_selection::{EnhancedSnapshot, SnapshotMenuEntry};
            use crate::menu_utils::{DialogOutcome, FzfWrapper};
            let entries: Vec<_> = snapshots
                .into_iter()
                .map(|snapshot| {
                    SnapshotMenuEntry::Snapshot(EnhancedSnapshot {
                        snapshot,
                        game_name: owner.name.clone(),
                        local_save_info: None,
                        nearest_checkpoint: owner.nearest_checkpoint.clone(),
                    })
                })
                .collect();
            match FzfWrapper::builder()
                .header(owner.scope())
                .items(entries)
                .select_one()?
            {
                DialogOutcome::Submitted(SnapshotMenuEntry::Snapshot(selected)) => {
                    selected.snapshot
                }
                _ => return Ok(()),
            }
        }
    };
    if !force
        && owner.pending_restore.is_none()
        && owner.checkpoint_matches(&snapshot)
        && crate::game::save_runtime::snapshot_digest(&snapshot)
            == Some(crate::game::save_runtime::content_digest(owner.save_path.as_path())?.as_str())
    {
        println!(
            "Restore skipped: {} already contains snapshot {}",
            owner.name, snapshot.short_id
        );
        return Ok(());
    }
    println!("{}", owner.scope());
    if !force
        && crate::menu_utils::FzfWrapper::confirm(&format!(
            "{}\nRestore snapshot {}? Existing local saves will be replaced.",
            owner.scope(),
            snapshot.short_id
        ))? != crate::menu_utils::ConfirmResult::Yes
    {
        return Ok(());
    }
    crate::game::operations::sync::execution::perform_restore(&owner, config, &snapshot.id)
}

fn select_game_name(name: Option<String>) -> Result<Option<String>> {
    match name {
        Some(name) => Ok(Some(name)),
        None => crate::game::games::selection::select_game_interactive(None),
    }
}

/// Handle restic command passthrough with instant games repository configuration
pub fn handle_restic_command(args: Vec<String>) -> Result<()> {
    // Load configuration
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;

    // Check restic availability and game manager initialization
    super::utils::validation::check_restic_and_game_manager(&game_config)?;

    // If no arguments provided, show help
    if args.is_empty() {
        let bin = env!("CARGO_BIN_NAME");
        eprintln!(
            "Error: No restic command provided.\n\n\
             Usage: {bin} game restic <restic-command> [args...]\n\n\
             Examples:\n\
             • {bin} game restic snapshots\n\
             • {bin} game restic backup --tag instantgame\n\
             • {bin} game restic stats\n\
             • {bin} game restic find .config\n\
             • {bin} game restic restore latest --target /tmp/restore-test",
        );
        return Err(anyhow::anyhow!("no restic command provided"));
    }

    // Initialize restic wrapper to get base command configuration
    let restic = crate::restic::ResticWrapper::new(
        game_config.repo.as_path().to_string_lossy().to_string(),
        game_config.repo_password.clone(),
    )
    .context("Failed to initialize restic wrapper")?;

    // Build restic command using centralized configuration
    let mut cmd = restic.base_command();

    // Add user-provided arguments
    cmd.args(&args);

    // Execute the command
    let output = cmd.output().context("Failed to execute restic command")?;

    // Print stdout to the user
    if !output.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }

    // Print stderr to the user
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    // Return appropriate result based on exit code
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "restic command failed with exit code: {}",
            output.status.code().unwrap_or(-1)
        ))
    }
}
