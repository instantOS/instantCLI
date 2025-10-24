use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeSet;

use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::game::games::manager::{AddGameOptions, GameManager};
use crate::game::games::validation::validate_game_manager_initialized;
use crate::menu::protocol;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

mod install;
mod paths;
mod restic;

pub fn setup_uninstalled_games() -> Result<()> {
    let mut game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    if !validate_game_manager_initialized()? {
        return Ok(());
    }

    loop {
        let snapshot_overview = restic::collect_snapshot_overview(&game_config)?;
        let candidates = collect_setup_candidates(&game_config, &installations, &snapshot_overview);

        if candidates.is_empty() {
            println!(
                "{} No games require setup. Use `ins game add` to add a new game.",
                char::from(NerdFont::Info)
            );
            return Ok(());
        }

        let selection = prompt_installation_choice(&candidates)?;

        match selection {
            Selection::Candidate(candidate) => {
                handle_candidate(candidate, &mut game_config, &mut installations)?;

                game_config =
                    InstantGameConfig::load().context("Failed to reload game configuration")?;
                installations = InstallationsConfig::load()
                    .context("Failed to reload installations configuration")?;
            }
            Selection::Manual => {
                GameManager::add_game(AddGameOptions::default())?;
                game_config =
                    InstantGameConfig::load().context("Failed to reload game configuration")?;
                installations = InstallationsConfig::load()
                    .context("Failed to reload installations configuration")?;
            }
            Selection::Done | Selection::Cancelled => break,
        }
    }

    Ok(())
}

#[derive(Clone)]
struct SetupCandidate {
    name: String,
    category: CandidateCategory,
    game: Option<Game>,
    installation: Option<GameInstallation>,
    snapshot: Option<restic::SnapshotOverview>,
    missing_dependencies: Vec<GameDependency>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum CandidateCategory {
    MissingInstallation,
    MissingDependencies,
    SnapshotWithoutGame,
    InstallationWithoutGame,
}

impl CandidateCategory {
    fn priority(self) -> u8 {
        match self {
            CandidateCategory::MissingInstallation => 0,
            CandidateCategory::MissingDependencies => 1,
            CandidateCategory::SnapshotWithoutGame => 2,
            CandidateCategory::InstallationWithoutGame => 3,
        }
    }
}

#[derive(Clone)]
enum Selection {
    Candidate(SetupCandidate),
    Manual,
    Done,
    Cancelled,
}

#[derive(Clone)]
struct CandidateOption {
    kind: CandidateOptionKind,
}

#[derive(Clone)]
enum CandidateOptionKind {
    Candidate(SetupCandidate),
    Manual,
    Done,
}

impl CandidateOption {
    fn candidate(candidate: SetupCandidate) -> Self {
        Self {
            kind: CandidateOptionKind::Candidate(candidate),
        }
    }

    fn manual() -> Self {
        Self {
            kind: CandidateOptionKind::Manual,
        }
    }

    fn done() -> Self {
        Self {
            kind: CandidateOptionKind::Done,
        }
    }
}

impl FzfSelectable for CandidateOption {
    fn fzf_display_text(&self) -> String {
        match &self.kind {
            CandidateOptionKind::Candidate(candidate) => candidate.summary_line(),
            CandidateOptionKind::Manual => {
                format!("{} Enter a new game manually", char::from(NerdFont::Edit))
            }
            CandidateOptionKind::Done => {
                format!("{} Finish game setup", char::from(NerdFont::Check))
            }
        }
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        match &self.kind {
            CandidateOptionKind::Candidate(candidate) => candidate.preview(),
            CandidateOptionKind::Manual => protocol::FzfPreview::Text(format!(
                "{} Choose this to run the interactive game addition flow.",
                char::from(NerdFont::Info)
            )),
            CandidateOptionKind::Done => {
                protocol::FzfPreview::Text("Exit the setup assistant.".to_string())
            }
        }
    }
}

impl SetupCandidate {
    fn summary_line(&self) -> String {
        let icon = char::from(NerdFont::Gamepad);
        match self.kind {
            CandidateKind::ResticOnly => {
                let snapshot_count = self
                    .snapshot
                    .as_ref()
                    .map(|overview| overview.snapshot_count)
                    .unwrap_or(0);
                let snapshot_label = if snapshot_count == 1 {
                    "snapshot"
                } else {
                    "snapshots"
                };
                format!(
                    "{icon} {}  — Backups detected ({snapshot_count} {snapshot_label})",
                    self.name
                )
            }
            CandidateKind::GameNeedsInstallation => {
                if self.snapshot.is_some() {
                    format!(
                        "{icon} {}  — Configure save path (backups available)",
                        self.name
                    )
                } else {
                    format!("{icon} {}  — Configure save path", self.name)
                }
            }
            CandidateKind::InstallationMissingGame => {
                format!("{icon} {}  — Add entry to games.toml", self.name)
            }
        }
    }

    fn preview(&self) -> protocol::FzfPreview {
        let mut sections = Vec::new();

        sections.push(format!(
            "{} GAME OVERVIEW\n\nName: {}\nStatus: {}",
            char::from(NerdFont::Gamepad),
            self.name,
            self.status_text()
        ));

        if let Some(game) = &self.game {
            let mut details = Vec::new();
            if let Some(desc) = game.description.as_deref()
                && !desc.trim().is_empty()
            {
                details.push(format!("Description: {}", desc.trim()));
            }
            if let Some(cmd) = game.launch_command.as_deref()
                && !cmd.trim().is_empty()
            {
                details.push(format!("Launch command: {}", cmd.trim()));
            }
            if !details.is_empty() {
                sections.push(details.join("\n"));
            }
        }

        if let Some(installation) = &self.installation {
            let mut info = Vec::new();
            info.push(format!(
                "{} Existing save path: {}",
                char::from(NerdFont::Folder),
                format_installation_path(installation)
            ));
            if let Some(checkpoint) = installation.nearest_checkpoint.as_deref() {
                info.push(format!("Nearest checkpoint: {}", checkpoint));
            }
            sections.push(info.join("\n"));
        }

        if let Some(snapshot) = &self.snapshot {
            let mut snapshot_info = Vec::new();
            snapshot_info.push(format!(
                "{} Restic snapshots: {}",
                char::from(NerdFont::Archive),
                snapshot.snapshot_count
            ));

            if let Some(time) = snapshot.latest_snapshot_time.as_deref().and_then(|iso| {
                restic::format_snapshot_timestamp(iso, snapshot.latest_snapshot_host.as_deref())
            }) {
                snapshot_info.push(format!("Latest backup: {}", time));
            }

            if !snapshot.hosts.is_empty() {
                let hosts = snapshot
                    .hosts
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                snapshot_info.push(format!("Hosts: {}", hosts));
            }

            if !snapshot.unique_paths.is_empty() {
                snapshot_info.push("Suggested save paths:".to_string());
                for path_info in snapshot.unique_paths.iter().take(6) {
                    snapshot_info.push(format!("  • {}", path_info.fzf_display_text()));
                }
                if snapshot.unique_paths.len() > 6 {
                    snapshot_info.push(format!(
                        "  • … and {} more",
                        snapshot.unique_paths.len() - 6
                    ));
                }
            }

            sections.push(snapshot_info.join("\n"));
        }

        protocol::FzfPreview::Text(sections.join("\n\n"))
    }

    fn status_text(&self) -> String {
        match self.kind {
            CandidateKind::ResticOnly => {
                "Backups detected (needs game and installation)".to_string()
            }
            CandidateKind::GameNeedsInstallation => {
                "Game registered (needs installation on this device)".to_string()
            }
            CandidateKind::InstallationMissingGame => {
                "Save path mapped locally but missing global configuration".to_string()
            }
        }
    }
}

fn collect_setup_candidates(
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
    snapshot_overview: &std::collections::HashMap<String, restic::SnapshotOverview>,
) -> Vec<SetupCandidate> {
    let mut names = BTreeSet::new();
    for game in &game_config.games {
        names.insert(game.name.0.clone());
    }
    for installation in &installations.installations {
        names.insert(installation.game_name.0.clone());
    }
    for name in snapshot_overview.keys() {
        names.insert(name.clone());
    }

    let mut candidates = Vec::new();

    for name in names {
        let game = game_config
            .games
            .iter()
            .find(|game| game.name.0 == name)
            .cloned();
        let installation = installations
            .installations
            .iter()
            .find(|inst| inst.game_name.0 == name)
            .cloned();
        let snapshot = snapshot_overview.get(&name).cloned();

        let has_game = game.is_some();
        let has_installation = installation.is_some();
        let has_snapshot = snapshot.is_some();

        let kind = if has_game && has_installation {
            continue;
        } else if has_installation && !has_game {
            CandidateKind::InstallationMissingGame
        } else if has_game && !has_installation {
            CandidateKind::GameNeedsInstallation
        } else if has_snapshot {
            CandidateKind::ResticOnly
        } else {
            continue;
        };

        candidates.push(SetupCandidate {
            name,
            kind,
            game,
            installation,
            snapshot,
        });
    }

    candidates
}

fn prompt_installation_choice(candidates: &[SetupCandidate]) -> Result<Selection> {
    let mut options: Vec<CandidateOption> = candidates
        .iter()
        .cloned()
        .map(CandidateOption::candidate)
        .collect();
    options.push(CandidateOption::manual());
    options.push(CandidateOption::done());

    let header = if candidates.is_empty() {
        format!(
            "{} No pending games detected. Choose an option below.",
            char::from(NerdFont::Info)
        )
    } else {
        format!(
            "{} Select a game to configure. Pending games: {}.",
            char::from(NerdFont::Info),
            candidates.len()
        )
    };

    match FzfWrapper::builder()
        .prompt("setup")
        .header(header)
        .select(options)?
    {
        FzfResult::Selected(option) => match option.kind {
            CandidateOptionKind::Candidate(candidate) => Ok(Selection::Candidate(candidate)),
            CandidateOptionKind::Manual => Ok(Selection::Manual),
            CandidateOptionKind::Done => Ok(Selection::Done),
        },
        FzfResult::Cancelled => Ok(Selection::Cancelled),
        FzfResult::Error(err) => Err(anyhow!(err)),
        FzfResult::MultiSelected(mut options) => {
            if let Some(option) = options.pop() {
                match option.kind {
                    CandidateOptionKind::Candidate(candidate) => {
                        Ok(Selection::Candidate(candidate))
                    }
                    CandidateOptionKind::Manual => Ok(Selection::Manual),
                    CandidateOptionKind::Done => Ok(Selection::Done),
                }
            } else {
                Ok(Selection::Cancelled)
            }
        }
    }
}

fn handle_candidate(
    candidate: SetupCandidate,
    game_config: &mut InstantGameConfig,
    installations: &mut InstallationsConfig,
) -> Result<()> {
    match candidate.kind {
        CandidateKind::ResticOnly => {
            ensure_game_entry(&candidate, game_config)?;
            install::setup_single_game(
                &candidate.name,
                game_config,
                installations,
                candidate.snapshot.as_ref(),
            )
        }
        CandidateKind::GameNeedsInstallation => install::setup_single_game(
            &candidate.name,
            game_config,
            installations,
            candidate.snapshot.as_ref(),
        ),
        CandidateKind::InstallationMissingGame => ensure_game_entry(&candidate, game_config),
    }
}

fn ensure_game_entry(
    candidate: &SetupCandidate,
    game_config: &mut InstantGameConfig,
) -> Result<()> {
    if candidate.game.is_some() {
        return Ok(());
    }

    println!(
        "{} Adding '{}' to games.toml...",
        char::from(NerdFont::Info),
        candidate.name
    );

    let description = GameManager::get_game_description()?;
    let launch_command = GameManager::get_launch_command()?;

    let mut game = Game::new(candidate.name.clone());

    if !description.trim().is_empty() {
        game.description = Some(description.trim().to_string());
    }

    if !launch_command.trim().is_empty() {
        game.launch_command = Some(launch_command.trim().to_string());
    }

    game_config.games.push(game);
    game_config.save()?;

    emit(
        Level::Success,
        "game.setup.game_added",
        &format!(
            "{} Added '{}' to games.toml",
            char::from(NerdFont::Check),
            candidate.name
        ),
        None,
    );

    Ok(())
}

fn format_installation_path(installation: &GameInstallation) -> String {
    installation
        .save_path
        .to_tilde_string()
        .unwrap_or_else(|_| {
            installation
                .save_path
                .as_path()
                .to_string_lossy()
                .to_string()
        })
}
