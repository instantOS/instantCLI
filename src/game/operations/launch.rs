use crate::ui::prelude::*;
use anyhow::Result;

/// Handle game launching
pub fn launch_game(game_name: String) -> Result<()> {
    emit(
        Level::Warn,
        "game.launch.unimplemented",
        &format!(
            "{} Launch command is not implemented yet.",
            char::from(Fa::Wrench)
        ),
        None,
    );
    emit(
        Level::Info,
        "game.launch.preview",
        &format!("{} Would launch game: {game_name}", char::from(Fa::Gamepad)),
        None,
    );

    Ok(())
}
