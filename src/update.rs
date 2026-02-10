use anyhow::{Context, Result};
use duct::cmd;
use nix::unistd::{AccessFlags, access};

use crate::common::deps::TOPGRADE;
use crate::common::package::{InstallResult, ensure_all};
use crate::dot;
use crate::game::operations::sync_game_saves;
use crate::self_update;
use crate::ui::prelude::*;

pub async fn handle_update_command(debug: bool) -> Result<()> {
    // 1. Ensure topgrade is installed
    match ensure_all(&[&TOPGRADE])? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => {}
        InstallResult::Declined => {
            emit(
                Level::Info,
                "update.cancelled",
                "System update cancelled.",
                None,
            );
            return Ok(());
        }
        InstallResult::NotAvailable { .. } | InstallResult::Failed { .. } => {
            emit(
                Level::Warn,
                "update.topgrade.missing",
                "Topgrade is required for system updates but was not installed.",
                None,
            );
            return Ok(());
        }
    }

    // 2. Run topgrade
    emit(
        Level::Info,
        "update.topgrade.start",
        "Starting system update with topgrade...",
        None,
    );

    let mut args = Vec::new();

    // Check if topgrade is at a root-owned location or otherwise not writable
    if let Ok(topgrade_path) = which::which("topgrade")
        && let Some(parent) = topgrade_path.parent()
    {
        let is_writable = access(parent, AccessFlags::W_OK).is_ok();
        if !is_writable {
            args.push("--no-self-update".to_string());
        }
    }

    // We use duct to run topgrade and stream output to stdout/stderr
    // topgrade is interactive so we don't capture output
    cmd("topgrade", args)
        .run()
        .context("Failed to run topgrade")?;

    emit(
        Level::Success,
        "update.topgrade.finish",
        "System update completed successfully",
        None,
    );

    // 3. Run ins dot update
    emit(
        Level::Info,
        "update.dot.start",
        "Updating dotfiles...",
        None,
    );

    let config = dot::config::DotfileConfig::load(None)?;
    config.ensure_directories()?;
    let db = dot::db::Database::new(config.database_path().to_path_buf())?;
    dot::update_all(&config, debug, &db, true)?;

    emit(
        Level::Success,
        "update.dot.finish",
        "Dotfiles updated successfully",
        None,
    );

    // 4. Run ins game sync
    emit(
        Level::Info,
        "update.game.start",
        "Syncing game saves...",
        None,
    );

    // We pass None for game_name to sync all games, and false for force
    let _summary = sync_game_saves(None, false)?;

    emit(
        Level::Success,
        "update.game.finish",
        "Game saves synced successfully",
        None,
    );

    // 5. Run ins self-update
    emit(
        Level::Info,
        "update.self.start",
        "Checking for CLI updates...",
        None,
    );

    self_update::self_update().await?;

    Ok(())
}
