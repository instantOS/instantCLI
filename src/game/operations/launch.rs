use crate::ui::prelude::*;
use anyhow::Result;

/// Handle game launching
pub fn launch_game(game_name: String) -> Result<()> {
    warn(
        "game.launch.unimplemented",
        "Launch command is not implemented yet.",
    );
    info(
        "game.launch.preview",
        &format!("Would launch game: {game_name}"),
    );

    Ok(())
}
