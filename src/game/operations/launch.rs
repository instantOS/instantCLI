use std::collections::{BTreeSet, HashMap};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};

use super::sync::sync_game_saves;

const POST_LAUNCH_SYNC_DELAY: Duration = Duration::from_secs(5);

/// Handle game launching
pub fn launch_game(game_name: Option<String>) -> Result<()> {
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    let launchables = collect_launchable_games(&game_config.games, &installations.installations);

    if launchables.is_empty() {
        println!(
            "No launch commands are configured. Add a launch command in games.toml or installations.toml."
        );
        return Ok(());
    }

    let selected = if let Some(requested) = game_name {
        match launchables.iter().find(|game| game.name == requested) {
            Some(game) => game.clone(),
            None => {
                eprintln!("No launch command configured for '{requested}'.");
                return Err(anyhow!("No launch command configured for '{}'.", requested));
            }
        }
    } else {
        match select_launchable_game(&launchables)? {
            Some(game) => game,
            None => return Ok(()),
        }
    };

    println!(
        "Launching {} using {}",
        selected.name,
        selected.source.label()
    );

    let _summary = sync_game_saves(None, false)?;

    run_launch_command(&selected)?;

    println!(
        "Waiting {} seconds before syncing saves...",
        POST_LAUNCH_SYNC_DELAY.as_secs()
    );
    sleep(POST_LAUNCH_SYNC_DELAY);

    let _summary = sync_game_saves(None, false)?;

    println!("Finished launch workflow for {}", selected.name);

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchableGame {
    name: String,
    effective_command: String,
    game_command: Option<String>,
    installation_command: Option<String>,
    source: LaunchCommandSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchCommandSource {
    Installation,
    GameConfig,
}

impl LaunchCommandSource {
    fn label(self) -> &'static str {
        match self {
            LaunchCommandSource::Installation => "installations.toml",
            LaunchCommandSource::GameConfig => "games.toml",
        }
    }
}

impl FzfSelectable for LaunchableGame {
    fn fzf_display_text(&self) -> String {
        format!("{} ({})", self.name, self.source.label())
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;

        let mut preview = format!(
            "Game: {}\nEffective source: {}\n",
            self.name,
            self.source.label()
        );

        if let Some(cmd) = &self.installation_command {
            preview.push_str(&format!("installations.toml: {cmd}\n"));
        }

        if let Some(cmd) = &self.game_command {
            preview.push_str(&format!("games.toml: {cmd}\n"));
        }

        preview.push_str(&format!("Effective command: {}", self.effective_command));

        FzfPreview::Text(preview)
    }

    fn fzf_key(&self) -> String {
        self.name.clone()
    }
}

fn collect_launchable_games(
    games: &[Game],
    installations: &[GameInstallation],
) -> Vec<LaunchableGame> {
    let mut games_by_name: HashMap<String, &Game> = HashMap::new();
    for game in games {
        games_by_name.insert(game.name.0.clone(), game);
    }

    let mut installations_by_name: HashMap<String, &GameInstallation> = HashMap::new();
    for installation in installations {
        installations_by_name.insert(installation.game_name.0.clone(), installation);
    }

    let mut names: BTreeSet<String> = BTreeSet::new();
    names.extend(games_by_name.keys().cloned());
    names.extend(installations_by_name.keys().cloned());

    let mut launchables = Vec::new();

    for name in names {
        let installation_command = installations_by_name
            .get(&name)
            .and_then(|installation| installation.launch_command.clone());
        let game_command = games_by_name
            .get(&name)
            .and_then(|game| game.launch_command.clone());

        let (effective_command, source) = if let Some(cmd) = installation_command.as_ref() {
            (cmd.clone(), LaunchCommandSource::Installation)
        } else if let Some(cmd) = game_command.as_ref() {
            (cmd.clone(), LaunchCommandSource::GameConfig)
        } else {
            continue;
        };

        launchables.push(LaunchableGame {
            name,
            effective_command,
            game_command,
            installation_command,
            source,
        });
    }

    launchables
}

fn select_launchable_game(launchables: &[LaunchableGame]) -> Result<Option<LaunchableGame>> {
    if launchables.is_empty() {
        return Ok(None);
    }

    if launchables.len() == 1 {
        return Ok(Some(launchables[0].clone()));
    }

    let result = FzfWrapper::builder()
        .prompt("Launch game")
        .header("Select a game to launch")
        .select(launchables.to_vec())?;

    match result {
        FzfResult::Selected(game) => Ok(Some(game)),
        FzfResult::Cancelled => Ok(None),
        FzfResult::Error(message) => Err(anyhow!(message)),
        FzfResult::MultiSelected(mut games) => Ok(games.pop()),
    }
}

fn run_launch_command(game: &LaunchableGame) -> Result<()> {
    println!("Running launch command for {}", game.name);

    let status = Command::new("sh")
        .arg("-c")
        .arg(&game.effective_command)
        .status()
        .with_context(|| {
            format!(
                "Failed to execute launch command '{}'.",
                game.effective_command
            )
        })?;

    if !status.success() {
        eprintln!(
            "Launch command exited with status {:?} for '{}'",
            status.code(),
            game.name
        );
        return Err(anyhow!(
            "Launch command exited with status {:?} for '{}'.",
            status.code(),
            game.name
        ));
    }

    println!("Launch command completed for {}", game.name);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::game::config::GameInstallation;
    use std::path::PathBuf;

    fn game(name: &str, launch_command: Option<&str>) -> Game {
        let mut g = Game::new(name);
        g.launch_command = launch_command.map(|cmd| cmd.to_string());
        g
    }

    fn installation(name: &str, launch_command: Option<&str>) -> GameInstallation {
        let mut inst =
            GameInstallation::new(name, TildePath::new(PathBuf::from("/tmp/instantcli-tests")));
        inst.launch_command = launch_command.map(|cmd| cmd.to_string());
        inst
    }

    #[test]
    fn installation_overrides_game_command() {
        let games = vec![game("Example", Some("game_cmd"))];
        let installations = vec![installation("Example", Some("install_cmd"))];

        let launchables = collect_launchable_games(&games, &installations);

        assert_eq!(launchables.len(), 1);
        let launchable = &launchables[0];
        assert_eq!(launchable.effective_command, "install_cmd");
        assert_eq!(launchable.source, LaunchCommandSource::Installation);
        assert_eq!(
            launchable.installation_command.as_deref(),
            Some("install_cmd")
        );
        assert_eq!(launchable.game_command.as_deref(), Some("game_cmd"));
    }

    #[test]
    fn game_command_used_when_installation_missing() {
        let games = vec![game("Example", Some("game_cmd"))];
        let installations: Vec<GameInstallation> = vec![];

        let launchables = collect_launchable_games(&games, &installations);

        assert_eq!(launchables.len(), 1);
        let launchable = &launchables[0];
        assert_eq!(launchable.effective_command, "game_cmd");
        assert_eq!(launchable.source, LaunchCommandSource::GameConfig);
        assert_eq!(launchable.installation_command, None);
        assert_eq!(launchable.game_command.as_deref(), Some("game_cmd"));
    }

    #[test]
    fn games_without_command_are_filtered_out() {
        let games = vec![game("NoCommand", None)];
        let installations = vec![installation("Other", None)];

        let launchables = collect_launchable_games(&games, &installations);

        assert!(launchables.is_empty());
    }
}
