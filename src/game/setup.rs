use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeSet;

use crate::game::config::{
    Game, GameDependency, GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind,
};
use crate::game::deps::manager::{InstallDependencyOptions, install_dependency};
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
        match self.category {
            CandidateCategory::MissingInstallation => {
                if self.snapshot.is_some() {
                    format!(
                        "{icon} {}  — Configure save path (backups available)",
                        self.name
                    )
                } else {
                    format!("{icon} {}  — Configure save path", self.name)
                }
            }
            CandidateCategory::MissingDependencies => {
                let missing = self.missing_dependencies.len();
                format!(
                    "{icon} {}  — Install {missing} pending {}",
                    self.name,
                    if missing == 1 {
                        "dependency"
                    } else {
                        "dependencies"
                    }
                )
            }
            CandidateCategory::SnapshotWithoutGame => {
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
            CandidateCategory::InstallationWithoutGame => {
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

        let mut pending = Vec::new();
        if self.game.is_none() {
            pending.push("• Missing games.toml entry".to_string());
        }
        if self.installation.is_none() {
            pending.push("• Save path not configured on this device".to_string());
        }
        if !self.missing_dependencies.is_empty() {
            let mut lines = vec!["• Missing dependencies:".to_string()];
            for dependency in &self.missing_dependencies {
                let label = if dependency.source_type.is_file() {
                    "file"
                } else {
                    "directory"
                };
                lines.push(format!("  ◦ {} ({label})", dependency.id));
            }
            pending.push(lines.join("\n"));
        }
        if !pending.is_empty() {
            sections.push(format!(
                "{} PENDING TASKS\n\n{}",
                char::from(NerdFont::Info),
                pending.join("\n")
            ));
        }

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
            if !installation.dependencies.is_empty() {
                let deps = installation
                    .dependencies
                    .iter()
                    .map(|dep| format!("  • {}", dep.dependency_id))
                    .collect::<Vec<_>>()
                    .join("\n");
                info.push(format!("Installed dependencies:\n{deps}"));
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
        match self.category {
            CandidateCategory::MissingInstallation => {
                "Game registered (needs installation on this device)".to_string()
            }
            CandidateCategory::MissingDependencies => {
                let missing = self.missing_dependencies.len();
                if missing == 1 {
                    "One dependency still needs installation".to_string()
                } else {
                    format!("{missing} dependencies still need installation")
                }
            }
            CandidateCategory::SnapshotWithoutGame => {
                "Backups detected (needs game entry and installation)".to_string()
            }
            CandidateCategory::InstallationWithoutGame => {
                "Save path mapped locally but missing games.toml entry".to_string()
            }
        }
    }
}

fn missing_dependencies_for_game(
    game: &Game,
    installation: Option<&GameInstallation>,
) -> Vec<GameDependency> {
    let installed: BTreeSet<String> = installation
        .map(|inst| {
            inst.dependencies
                .iter()
                .map(|dep| dep.dependency_id.clone())
                .collect()
        })
        .unwrap_or_default();

    game.dependencies
        .iter()
        .filter(|dependency| !installed.contains(&dependency.id))
        .cloned()
        .collect()
}

fn collect_setup_candidates(
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
    snapshot_overview: &std::collections::HashMap<String, restic::SnapshotOverview>,
) -> Vec<SetupCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();

    for game in &game_config.games {
        let name = game.name.0.clone();
        let installation = installations
            .installations
            .iter()
            .find(|inst| inst.game_name.0 == name)
            .cloned();
        let snapshot = snapshot_overview.get(&name).cloned();
        let missing_dependencies = missing_dependencies_for_game(game, installation.as_ref());

        if installation.is_none() {
            candidates.push(SetupCandidate {
                name: name.clone(),
                category: CandidateCategory::MissingInstallation,
                game: Some(game.clone()),
                installation: None,
                snapshot,
                missing_dependencies: missing_dependencies.clone(),
            });
            seen.insert(name);
            continue;
        }

        if !missing_dependencies.is_empty() {
            candidates.push(SetupCandidate {
                name: name.clone(),
                category: CandidateCategory::MissingDependencies,
                game: Some(game.clone()),
                installation: installation.clone(),
                snapshot: snapshot.clone(),
                missing_dependencies,
            });
        }

        seen.insert(name);
    }

    for (name, snapshot) in snapshot_overview {
        if seen.contains(name) {
            continue;
        }

        if game_config.games.iter().any(|game| &game.name.0 == name) {
            continue;
        }

        candidates.push(SetupCandidate {
            name: name.clone(),
            category: CandidateCategory::SnapshotWithoutGame,
            game: None,
            installation: None,
            snapshot: Some(snapshot.clone()),
            missing_dependencies: Vec::new(),
        });
        seen.insert(name.clone());
    }

    for installation in &installations.installations {
        let name = installation.game_name.0.clone();
        if seen.contains(&name) {
            continue;
        }

        if game_config.games.iter().any(|game| game.name.0 == name) {
            continue;
        }

        candidates.push(SetupCandidate {
            name: name.clone(),
            category: CandidateCategory::InstallationWithoutGame,
            game: None,
            installation: Some(installation.clone()),
            snapshot: snapshot_overview.get(&name).cloned(),
            missing_dependencies: Vec::new(),
        });
    }

    candidates.sort_by(|a, b| {
        a.category
            .priority()
            .cmp(&b.category.priority())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    candidates
}

#[derive(Clone)]
enum SetupTask {
    ConfigureSavePath,
    ConfigureDependency {
        id: String,
        source_type: PathContentKind,
    },
}

impl SetupTask {
    fn label(&self, game_name: &str) -> String {
        match self {
            SetupTask::ConfigureSavePath => format!("Configure save path for '{game_name}'"),
            SetupTask::ConfigureDependency { id, .. } => {
                format!("Install dependency '{id}'")
            }
        }
    }

    fn preview(&self, game_name: &str) -> String {
        match self {
            SetupTask::ConfigureSavePath => format!(
                "{} Configure the save location for '{game_name}'.

Selecting this will prompt for the correct save path and optionally restore the latest backup.",
                char::from(NerdFont::Folder)
            ),
            SetupTask::ConfigureDependency { id, source_type } => {
                let kind = if source_type.is_file() {
                    "file"
                } else {
                    "directory"
                };
                format!(
                    "{} Install dependency '{id}'.

The dependency stores a {kind} and will be restored from the latest backup if available.",
                    char::from(NerdFont::Info)
                )
            }
        }
    }
}

fn gather_pending_tasks(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
) -> Vec<SetupTask> {
    let game = game_config
        .games
        .iter()
        .find(|game| game.name.0 == game_name);
    let installation = installations
        .installations
        .iter()
        .find(|inst| inst.game_name.0 == game_name);

    let mut tasks = Vec::new();

    if installation.is_none() {
        if game.is_some() {
            tasks.push(SetupTask::ConfigureSavePath);
        }
        return tasks;
    }

    if let Some(game) = game {
        let missing = missing_dependencies_for_game(game, installation);
        for dependency in missing {
            tasks.push(SetupTask::ConfigureDependency {
                id: dependency.id.clone(),
                source_type: dependency.source_type,
            });
        }
    }

    tasks
}

fn prompt_task_choice(game_name: &str, tasks: &[SetupTask]) -> Result<Option<SetupTask>> {
    let options: Vec<TaskOption> = tasks
        .iter()
        .cloned()
        .map(|task| TaskOption::new(game_name, task))
        .collect();

    match FzfWrapper::builder()
        .prompt("step")
        .header(format!(
            "{} Select the next setup step for '{game_name}'.",
            char::from(NerdFont::Info)
        ))
        .select(options)?
    {
        FzfResult::Selected(option) => Ok(Some(option.task)),
        FzfResult::MultiSelected(mut options) => Ok(options.pop().map(|opt| opt.task)),
        FzfResult::Cancelled => Ok(None),
        FzfResult::Error(err) => Err(anyhow!(err)),
    }
}

#[derive(Clone)]
struct TaskOption {
    label: String,
    preview: String,
    task: SetupTask,
}

impl TaskOption {
    fn new(game_name: &str, task: SetupTask) -> Self {
        let label = task.label(game_name);
        let preview = task.preview(game_name);
        Self {
            label,
            preview,
            task,
        }
    }
}

impl FzfSelectable for TaskOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        protocol::FzfPreview::Text(self.preview.clone())
    }
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
    let game_name = candidate.name.clone();

    if candidate.game.is_none() {
        ensure_game_entry(&candidate, game_config)?;
        *game_config = InstantGameConfig::load().context("Failed to reload game configuration")?;
    }

    loop {
        let tasks = gather_pending_tasks(&game_name, game_config, installations);
        if tasks.is_empty() {
            break;
        }

        let task = if tasks.len() == 1 {
            tasks.into_iter().next().unwrap()
        } else {
            match prompt_task_choice(&game_name, &tasks)? {
                Some(task) => task,
                None => break,
            }
        };

        match task {
            SetupTask::ConfigureSavePath => {
                let snapshot_map = restic::collect_snapshot_overview(game_config)?;
                let snapshot = snapshot_map.get(&game_name).cloned();
                install::setup_single_game(
                    &game_name,
                    game_config,
                    installations,
                    snapshot.as_ref(),
                )?;
            }
            SetupTask::ConfigureDependency { id, .. } => {
                install_dependency(InstallDependencyOptions {
                    game_name: Some(game_name.clone()),
                    dependency_id: Some(id),
                    install_path: None,
                })?;
            }
        }

        *game_config = InstantGameConfig::load().context("Failed to reload game configuration")?;
        *installations =
            InstallationsConfig::load().context("Failed to reload installations configuration")?;
    }

    Ok(())
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
