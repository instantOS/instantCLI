use crate::fzf_wrapper::{FzfSelectable, FzfWrapper};
use crate::game::config::{Game, InstantGameConfig};
use crate::menu::protocol::FzfPreview;
use anyhow::{Context, Result};

impl FzfSelectable for Game {
    fn fzf_display_text(&self) -> String {
        self.name.0.clone()
    }

    fn fzf_key(&self) -> String {
        self.name.0.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        match &self.description {
            Some(desc) => FzfPreview::Text(desc.clone()),
            None => FzfPreview::None,
        }
    }
}

/// Helper function to select a game interactively
/// Returns Some(game_name) if a game was selected, None if cancelled
pub fn select_game_interactive(prompt_message: Option<&str>) -> Result<Option<String>> {
    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    if config.games.is_empty() {
        println!("No games configured yet.");
        println!("Use 'instant game add' to add a game.");
        return Ok(None);
    }

    // Show FZF menu to select game
    if let Some(message) = prompt_message {
        FzfWrapper::message(message).context("Failed to show selection prompt")?;
    }
    let selected = FzfWrapper::select_one(config.games.clone())
        .map_err(|e| anyhow::anyhow!("Failed to select game: {}", e))?;

    match selected {
        Some(game) => Ok(Some(game.name.0)),
        None => {
            println!("No game selected.");
            Ok(None)
        }
    }
}
