use anyhow::{Context, Result};
use colored::Colorize;
use duct::cmd;

use crate::common::requirements::TOPGRADE_PACKAGE;
use crate::dot;
use crate::game::operations::sync_game_saves;
use crate::ui::prelude::*;

pub async fn handle_update_command(debug: bool) -> Result<()> {
    // 1. Ensure topgrade is installed
    if !TOPGRADE_PACKAGE.ensure()? {
        emit(
            Level::Warn,
            "update.topgrade.missing",
            "Topgrade is required for system updates but was not installed.",
            None,
        );
        return Ok(());
    }

    // 2. Run topgrade
    emit(
        Level::Info,
        "update.topgrade.start",
        "Starting system update with topgrade...",
        None,
    );

    // We use duct to run topgrade and stream output to stdout/stderr
    // topgrade is interactive so we don't capture output
    cmd("topgrade", Vec::<String>::new())
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

    dot::update_all(&dot::config::Config::load(None)?, debug)?;

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
    sync_game_saves(None, false)?;

    emit(
        Level::Success,
        "update.game.finish",
        "Game saves synced successfully",
        None,
    );

    Ok(())
}
