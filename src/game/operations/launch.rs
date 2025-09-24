use anyhow::{Context, Result};
use crate::fzf_wrapper::FzfWrapper;

/// Handle game launching
pub fn launch_game(game_name: String) -> Result<()> {
    FzfWrapper::message(&format!(
        "Launch command not yet implemented\n\nWould launch game: {}",
        game_name
    )).context("Failed to show not implemented message")?;

    Ok(())
}