use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use std::collections::{BTreeSet, HashMap};

use crate::game::config::{
    Game, GameDependency, GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind,
};
use crate::game::deps::manager::{InstallDependencyOptions, install_dependency};
use crate::game::games::prompts;
use crate::game::games::validation::validate_game_manager_initialized;
use crate::game::games::{AddGameOptions, GameManager};
use crate::game::platforms::{LaunchCommandBuilderContext, build_optional_launch_command};
use crate::menu::protocol;
use crate::menu_utils::{DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use install::SetupStepOutcome;

mod install;
mod paths;
mod restic;

pub fn setup_uninstalled_games() -> Result<()> {
    let mut game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let initialized_here = !game_config.is_initialized();
    if initialized_here {
        use crate::game::repository::manager::{GameRepositoryManager, InitOptions, InitOutcome};
        if GameRepositoryManager::initialize_game_manager(false, InitOptions::default())?
            == InitOutcome::Cancelled
        {
            return Ok(());
        }
        game_config = InstantGameConfig::load().context("Failed to reload game configuration")?;
    }
    if !crate::game::reconciliation::reconcile_configured_games(&game_config)? {
        return Ok(());
    }
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;
    // The list reopens after every finished or cancelled task; keep the
    // cursor on the game the user was working on.
    let mut cursor = MenuCursor::new();

    loop {
        let snapshot_overview = restic::collect_snapshot_overview(&game_config)?;
        let candidates =
            CandidateCollector::new(&game_config, &installations, &snapshot_overview).collect();

        if candidates.is_empty() {
            println!(
                "{} No games require setup. Use `ins game add` to add a new game.",
                char::from(NerdFont::Info)
            );
            if initialized_here
                && FzfWrapper::confirm("Backup storage is ready. Add a game now?")?
                    == crate::menu_utils::ConfirmResult::Yes
            {
                GameManager::add_game(AddGameOptions::default())?;
            }
            return Ok(());
        }

        let selection = prompt_installation_choice(&candidates, &mut cursor)?;

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
            Selection::Back | Selection::Cancelled => break,
        }
    }

    Ok(())
}

pub fn setup_tracked_game(game_name: &str, discovered_save_path: Option<&str>) -> Result<()> {
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    if !validate_game_manager_initialized()? {
        return Ok(());
    }

    if !game_config
        .games
        .iter()
        .any(|game| game.name.0 == game_name)
    {
        return Err(anyhow!("Game '{}' not found in games.toml", game_name));
    }

    if installations
        .installations
        .iter()
        .any(|inst| inst.game_name.0 == game_name)
    {
        return crate::game::menu::game_menu(Some(game_name.to_string()));
    }

    if let crate::game::ownership::OwnerId::Group(id) = game_config.owner_id(game_name) {
        if game_config
            .resolve_owner(&installations, &game_config.owner_id(game_name))
            .is_ok()
        {
            return crate::game::menu::game_menu(Some(game_name.into()));
        }
        anyhow::bail!(
            "Bind shared save group '{id}' on this device with ins game group bind before setting up this game"
        );
    }
    let snapshot_overview = restic::collect_snapshot_overview(&game_config)?;
    let snapshot = snapshot_overview.get(game_name);

    install::setup_single_game_with_discovered_path(
        game_name,
        &game_config,
        &mut installations,
        snapshot,
        if snapshot.is_none() {
            discovered_save_path
        } else {
            None
        },
    )?;

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

    /// The icon shown in the badge of the menu row and in the preview.
    fn icon(self) -> NerdFont {
        match self {
            CandidateCategory::MissingInstallation => NerdFont::Folder,
            CandidateCategory::MissingDependencies => NerdFont::Package,
            CandidateCategory::SnapshotWithoutGame => NerdFont::BackupRestore,
            CandidateCategory::InstallationWithoutGame => NerdFont::FileConfig,
        }
    }

    fn color(self) -> &'static str {
        match self {
            CandidateCategory::MissingInstallation => colors::PEACH,
            CandidateCategory::MissingDependencies => colors::YELLOW,
            CandidateCategory::SnapshotWithoutGame => colors::BLUE,
            CandidateCategory::InstallationWithoutGame => colors::MAUVE,
        }
    }
}

#[derive(Clone)]
enum Selection {
    Candidate(SetupCandidate),
    Manual,
    Back,
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
    Back,
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

    fn back() -> Self {
        Self {
            kind: CandidateOptionKind::Back,
        }
    }
}

/// Rows of the setup list: every candidate, then the manual entry and the
/// way back to the game menu.
fn candidate_options(candidates: &[SetupCandidate]) -> Vec<CandidateOption> {
    let mut options: Vec<CandidateOption> = candidates
        .iter()
        .cloned()
        .map(CandidateOption::candidate)
        .collect();
    options.push(CandidateOption::manual());
    options.push(CandidateOption::back());
    options
}

impl FzfSelectable for CandidateOption {
    fn fzf_display_text(&self) -> String {
        match &self.kind {
            CandidateOptionKind::Candidate(candidate) => candidate.summary_line(),
            CandidateOptionKind::Manual => format!(
                "{} Enter a new game manually",
                format_icon_colored(NerdFont::Edit, colors::BLUE)
            ),
            CandidateOptionKind::Back => format!("{} Back", format_back_icon()),
        }
    }

    /// A candidate is identified by its game, so the cursor can follow it
    /// when its pending task changes and its label with it.
    fn fzf_key(&self) -> String {
        match &self.kind {
            CandidateOptionKind::Candidate(candidate) => candidate.name.clone(),
            CandidateOptionKind::Manual => "!__manual__".to_string(),
            CandidateOptionKind::Back => "!__back__".to_string(),
        }
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        match &self.kind {
            CandidateOptionKind::Candidate(candidate) => candidate.preview(),
            CandidateOptionKind::Manual => PreviewBuilder::new()
                .header(NerdFont::Edit, "Manual Entry")
                .text("Enter the game's details by hand.")
                .blank()
                .text("You will be prompted for:")
                .bullet("Game name")
                .bullet("Description (optional)")
                .bullet("Launch command (optional)")
                .bullet("Save data path")
                .build(),
            CandidateOptionKind::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Leave the setup assistant.")
                .blank()
                .subtext("Games that still need setup are offered again next time.")
                .build(),
        }
    }
}

impl SetupCandidate {
    /// Menu row: a badge for the kind of setup, the game, and what it needs.
    fn summary_line(&self) -> String {
        let category = self.category;
        format!(
            "{} {}  {}",
            format_icon_colored(category.icon(), category.color()),
            self.name,
            format_with_color(&format!("— {}", self.task_hint()), category.color())
        )
    }

    /// Short description of the setup this candidate still needs.
    fn task_hint(&self) -> String {
        match self.category {
            CandidateCategory::MissingInstallation => {
                if self.snapshot.is_some() {
                    "Configure save path (backups available)".to_string()
                } else {
                    "Configure save path".to_string()
                }
            }
            CandidateCategory::MissingDependencies => {
                let missing = self.missing_dependencies.len();
                format!(
                    "Install {missing} pending {}",
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
                format!("Backups detected ({snapshot_count} {snapshot_label})")
            }
            CandidateCategory::InstallationWithoutGame => "Add entry to games.toml".to_string(),
        }
    }

    fn preview(&self) -> protocol::FzfPreview {
        protocol::FzfPreview::Text(self.build_preview())
    }

    /// What choosing this candidate leads to.
    fn next_step(&self) -> String {
        match self.category {
            CandidateCategory::MissingInstallation if self.snapshot.is_some() => {
                "Press Enter to choose the save path, then decide between the backup and local saves."
            }
            CandidateCategory::MissingInstallation => {
                "Press Enter to choose where this game stores its saves."
            }
            CandidateCategory::MissingDependencies if self.missing_dependencies.len() > 1 => {
                "Press Enter to choose which dependency to install first."
            }
            CandidateCategory::MissingDependencies => {
                "Press Enter to install the missing dependency."
            }
            CandidateCategory::SnapshotWithoutGame => {
                "Press Enter to register the game, choose its save path and restore a backup."
            }
            CandidateCategory::InstallationWithoutGame => {
                "Press Enter to register the game with a description and launch command."
            }
        }
        .to_string()
    }

    fn build_preview(&self) -> String {
        let builder = PreviewBuilder::new()
            .header(self.category.icon(), &self.name)
            .line(
                self.category.color(),
                Some(self.category.icon()),
                &self.status_text(),
            );
        let builder = self.add_pending_tasks(builder);
        let builder = self.add_game_details(builder);
        let builder = self.add_installation(builder);
        let builder = self.add_snapshots(builder);

        builder.blank().subtext(&self.next_step()).build_string()
    }

    fn add_pending_tasks(&self, mut builder: PreviewBuilder) -> PreviewBuilder {
        let mut tasks = Vec::new();
        if self.game.is_none() {
            tasks.push("Missing games.toml entry".to_string());
        }
        if self.installation.is_none() {
            tasks.push("Save path not configured on this device".to_string());
        }
        if !self.missing_dependencies.is_empty() {
            let dependencies = self
                .missing_dependencies
                .iter()
                .map(|dependency| {
                    format!("{} ({})", dependency.id, kind_label(dependency.source_type))
                })
                .collect::<Vec<_>>()
                .join(", ");
            tasks.push(format!("Missing dependencies: {dependencies}"));
        }
        if tasks.is_empty() {
            return builder;
        }

        builder = builder
            .blank()
            .separator()
            .line(colors::YELLOW, Some(NerdFont::Warning), "Pending Tasks")
            .blank();
        for task in &tasks {
            builder = builder.bullet(task);
        }
        builder
    }

    fn add_game_details(&self, mut builder: PreviewBuilder) -> PreviewBuilder {
        let Some(game) = self.game.as_ref() else {
            return builder;
        };
        let description = game
            .description
            .as_deref()
            .map(str::trim)
            .filter(|description| !description.is_empty());
        let command = game
            .launch_command
            .as_ref()
            .map(ToString::to_string)
            .map(|command| command.trim().to_string())
            .filter(|command| !command.is_empty());
        if description.is_none() && command.is_none() {
            return builder;
        }

        builder = builder
            .blank()
            .separator()
            .blank()
            .line(colors::MAUVE, Some(NerdFont::Gamepad), "Game")
            .blank();
        if let Some(description) = description {
            builder = builder.field_indented("Description", description);
        }
        if let Some(command) = &command {
            builder = builder.field_indented("Launch command", command);
        }
        builder
    }

    fn add_installation(&self, mut builder: PreviewBuilder) -> PreviewBuilder {
        let Some(installation) = self.installation.as_ref() else {
            return builder;
        };
        let Ok(saves) = installation.owned() else {
            return builder;
        };

        builder = builder
            .blank()
            .separator()
            .blank()
            .line(colors::MAUVE, Some(NerdFont::Desktop), "This Device")
            .blank()
            .field_indented("Save Path", &saves.save_path.display_string());

        if let Some(checkpoint) = saves.nearest_checkpoint.as_deref() {
            builder = builder.field_indented("Checkpoint", checkpoint);
        }

        if !installation.dependencies.is_empty() {
            let installed = installation
                .dependencies
                .iter()
                .map(|dependency| dependency.dependency_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            builder = builder.field_indented("Dependencies", &installed);
        }

        builder
    }

    fn add_snapshots(&self, mut builder: PreviewBuilder) -> PreviewBuilder {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return builder;
        };

        builder = builder
            .blank()
            .separator()
            .blank()
            .line(colors::MAUVE, Some(NerdFont::BackupRestore), "Backups")
            .blank()
            .field_indented("Snapshots", &snapshot.snapshot_count.to_string());

        if let Some(timestamp) = snapshot.latest_snapshot_time.as_deref().and_then(|iso| {
            restic::format_snapshot_timestamp(iso, snapshot.latest_snapshot_host.as_deref())
        }) {
            builder = builder.field_indented("Latest backup", &timestamp);
        }

        if !snapshot.hosts.is_empty() {
            let hosts = snapshot
                .hosts
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            builder = builder.field_indented("Hosts", &hosts);
        }

        if !snapshot.unique_paths.is_empty() {
            const MAX_SUGGESTED_PATHS: usize = 6;
            builder = builder.blank().subtext("Suggested save paths:");
            for path_info in snapshot.unique_paths.iter().take(MAX_SUGGESTED_PATHS) {
                builder = builder.bullet(&path_info.summary());
            }
            if snapshot.unique_paths.len() > MAX_SUGGESTED_PATHS {
                builder = builder.subtext(&format!(
                    "  … and {} more",
                    snapshot.unique_paths.len() - MAX_SUGGESTED_PATHS
                ));
            }
        }

        builder
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

    fn icon(&self) -> NerdFont {
        match self {
            SetupTask::ConfigureSavePath => NerdFont::Folder,
            SetupTask::ConfigureDependency { .. } => NerdFont::Package,
        }
    }

    fn color(&self) -> &'static str {
        match self {
            SetupTask::ConfigureSavePath => colors::PEACH,
            SetupTask::ConfigureDependency { .. } => colors::YELLOW,
        }
    }

    fn build_preview(&self, game_name: &str) -> String {
        match self {
            SetupTask::ConfigureSavePath => PreviewBuilder::new()
                .header(NerdFont::Folder, "Configure Save Path")
                .text(&format!("Configure the save location for '{game_name}'."))
                .blank()
                .text("Selecting this will prompt for the correct save path")
                .text("and optionally restore the latest backup.")
                .build_string(),
            SetupTask::ConfigureDependency { id, source_type } => {
                let kind = kind_label(*source_type);
                PreviewBuilder::new()
                    .header(NerdFont::Package, "Install Dependency")
                    .field("Dependency", id)
                    .field("Stores", kind)
                    .blank()
                    .text(&format!(
                        "The dependency stores a {kind} and will be restored"
                    ))
                    .text("from the latest backup if available.")
                    .build_string()
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

    let selection = FzfWrapper::builder()
        .prompt("Select")
        .header(
            HeaderBuilder::new(NerdFont::Wrench, format!("Set up '{game_name}'"))
                .subtitle("Choose the next setup step.")
                .build(),
        )
        .responsive_layout()
        .items(options)
        .padded()
        .select_one()?;

    Ok(match selection {
        DialogOutcome::Submitted(option) => Some(option.task),
        DialogOutcome::Cancelled => None,
    })
}

#[derive(Clone)]
struct TaskOption {
    game_name: String,
    task: SetupTask,
}

impl TaskOption {
    fn new(game_name: &str, task: SetupTask) -> Self {
        Self {
            game_name: game_name.to_string(),
            task,
        }
    }
}

impl FzfSelectable for TaskOption {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} {}",
            format_icon_colored(self.task.icon(), self.task.color()),
            self.task.label(&self.game_name)
        )
    }

    fn fzf_key(&self) -> String {
        match &self.task {
            SetupTask::ConfigureSavePath => "save-path".to_string(),
            SetupTask::ConfigureDependency { id, .. } => format!("dependency:{id}"),
        }
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        protocol::FzfPreview::Text(self.task.build_preview(&self.game_name))
    }
}

fn prompt_installation_choice(
    candidates: &[SetupCandidate],
    cursor: &mut MenuCursor,
) -> Result<Selection> {
    let options = candidate_options(candidates);

    let subtitle = if candidates.is_empty() {
        "No pending games detected. Choose an option below.".to_string()
    } else {
        format!(
            "{} {} setup.",
            candidates.len(),
            if candidates.len() == 1 {
                "game needs"
            } else {
                "games need"
            }
        )
    };
    let header = HeaderBuilder::new(NerdFont::Wrench, "Set Up Existing Games")
        .subtitle(subtitle)
        .build();

    let mut builder = FzfWrapper::builder()
        .prompt("Select")
        .header(header)
        .responsive_layout();
    if let Some(index) = cursor.initial_index(&options) {
        builder = builder.initial_index(index);
    }

    let selection = builder.items(options.clone()).padded().select_one()?;

    Ok(match selection {
        DialogOutcome::Submitted(option) => {
            cursor.update(&option, &options);
            match option.kind {
                CandidateOptionKind::Candidate(candidate) => Selection::Candidate(candidate),
                CandidateOptionKind::Manual => Selection::Manual,
                CandidateOptionKind::Back => Selection::Back,
            }
        }
        DialogOutcome::Cancelled => Selection::Cancelled,
    })
}

fn handle_candidate(
    candidate: SetupCandidate,
    game_config: &mut InstantGameConfig,
    installations: &mut InstallationsConfig,
) -> Result<()> {
    let game_name = candidate.name.clone();

    if candidate.game.is_none() {
        if !ensure_game_entry(&candidate, game_config)? {
            return Ok(());
        }
        *game_config = InstantGameConfig::load().context("Failed to reload game configuration")?;
    }

    let tasks = gather_pending_tasks(&game_name, game_config, installations);
    if tasks.is_empty() {
        return Ok(());
    }

    let task = if tasks.len() == 1 {
        tasks.into_iter().next().unwrap()
    } else {
        match prompt_task_choice(&game_name, &tasks)? {
            Some(task) => task,
            None => return Ok(()),
        }
    };

    let outcome = match task {
        SetupTask::ConfigureSavePath => {
            let snapshot_map = restic::collect_snapshot_overview(game_config)?;
            let snapshot = snapshot_map.get(&game_name).cloned();
            install::setup_single_game(&game_name, game_config, installations, snapshot.as_ref())?
        }
        SetupTask::ConfigureDependency { id, .. } => {
            install_dependency(InstallDependencyOptions {
                game_name: Some(game_name.clone()),
                dependency_id: Some(id),
                install_path: None,
            })?;
            SetupStepOutcome::Completed
        }
    };

    *game_config = InstantGameConfig::load().context("Failed to reload game configuration")?;
    *installations =
        InstallationsConfig::load().context("Failed to reload installations configuration")?;

    if outcome == SetupStepOutcome::Completed {
        // Task finished; return to main menu for a cleaner UX.
        return Ok(());
    }

    Ok(())
}

fn ensure_game_entry(
    candidate: &SetupCandidate,
    game_config: &mut InstantGameConfig,
) -> Result<bool> {
    if candidate.game.is_some() {
        return Ok(true);
    }

    println!(
        "{} Adding '{}' to games.toml...",
        char::from(NerdFont::Info),
        candidate.name
    );

    let description = prompts::get_game_description()?;
    let context = LaunchCommandBuilderContext::from_game(
        candidate
            .installation
            .as_ref()
            .and_then(|installation| installation.saves.as_ref())
            .map(|saves| saves.save_path.as_path()),
        candidate
            .installation
            .as_ref()
            .and_then(|installation| installation.launch_command.as_ref()),
    );
    let launch_command = match build_optional_launch_command(Some(&context))? {
        crate::menu_utils::DialogOutcome::Submitted(command) => command,
        crate::menu_utils::DialogOutcome::Cancelled => return Ok(false),
    };

    let mut game = Game::new(candidate.name.clone());

    if !description.trim().is_empty() {
        game.description = Some(description.trim().to_string());
    }

    game.launch_command = launch_command;

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

    Ok(true)
}

/// Human-readable label for the kind of content a dependency stores.
fn kind_label(kind: PathContentKind) -> &'static str {
    match kind {
        PathContentKind::Directory => "directory",
        PathContentKind::File => "file",
    }
}

struct CandidateCollector<'a> {
    game_config: &'a InstantGameConfig,
    installations: &'a InstallationsConfig,
    snapshot_overview: &'a HashMap<String, restic::SnapshotOverview>,
    candidates: Vec<SetupCandidate>,
    seen: BTreeSet<String>,
}

impl<'a> CandidateCollector<'a> {
    fn new(
        game_config: &'a InstantGameConfig,
        installations: &'a InstallationsConfig,
        snapshot_overview: &'a HashMap<String, restic::SnapshotOverview>,
    ) -> Self {
        Self {
            game_config,
            installations,
            snapshot_overview,
            candidates: Vec::new(),
            seen: BTreeSet::new(),
        }
    }

    fn collect(mut self) -> Vec<SetupCandidate> {
        self.add_registered_game_candidates();
        self.add_snapshot_only_candidates();
        self.add_installation_only_candidates();
        self.finalize()
    }

    fn add_registered_game_candidates(&mut self) {
        for game in &self.game_config.games {
            let name = game.name.0.clone();
            let installation = self
                .installations
                .installations
                .iter()
                .find(|inst| inst.game_name.0 == name)
                .cloned();
            let snapshot = self.snapshot_overview.get(&name).cloned();
            let missing_dependencies = missing_dependencies_for_game(game, installation.as_ref());

            if installation.is_none() && game.save_group.is_none() {
                self.push_candidate(SetupCandidate {
                    name: name.clone(),
                    category: CandidateCategory::MissingInstallation,
                    game: Some(game.clone()),
                    installation: None,
                    snapshot,
                    missing_dependencies: missing_dependencies.clone(),
                });
                self.seen.insert(name);
                continue;
            }

            if !missing_dependencies.is_empty() {
                self.push_candidate(SetupCandidate {
                    name: name.clone(),
                    category: CandidateCategory::MissingDependencies,
                    game: Some(game.clone()),
                    installation: installation.clone(),
                    snapshot: snapshot.clone(),
                    missing_dependencies,
                });
            }

            self.seen.insert(name);
        }
    }

    fn add_snapshot_only_candidates(&mut self) {
        for (name, snapshot) in self.snapshot_overview {
            if self.seen.contains(name) {
                continue;
            }

            if self
                .game_config
                .games
                .iter()
                .any(|game| &game.name.0 == name)
            {
                continue;
            }

            self.push_candidate(SetupCandidate {
                name: name.clone(),
                category: CandidateCategory::SnapshotWithoutGame,
                game: None,
                installation: None,
                snapshot: Some(snapshot.clone()),
                missing_dependencies: Vec::new(),
            });
        }
    }

    fn add_installation_only_candidates(&mut self) {
        for installation in &self.installations.installations {
            let name = installation.game_name.0.clone();
            if self.seen.contains(&name) {
                continue;
            }

            if self
                .game_config
                .games
                .iter()
                .any(|game| game.name.0 == name)
            {
                continue;
            }

            self.push_candidate(SetupCandidate {
                name: name.clone(),
                category: CandidateCategory::InstallationWithoutGame,
                game: None,
                installation: Some(installation.clone()),
                snapshot: self.snapshot_overview.get(&name).cloned(),
                missing_dependencies: Vec::new(),
            });
        }
    }

    fn push_candidate(&mut self, candidate: SetupCandidate) {
        self.seen.insert(candidate.name.clone());
        self.candidates.push(candidate);
    }

    fn finalize(mut self) -> Vec<SetupCandidate> {
        self.candidates.sort_by(|a, b| {
            a.category
                .priority()
                .cmp(&b.category.priority())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        self.candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};
    use crate::ui::text::strip_ansi;

    const CATEGORIES: [CandidateCategory; 4] = [
        CandidateCategory::MissingInstallation,
        CandidateCategory::MissingDependencies,
        CandidateCategory::SnapshotWithoutGame,
        CandidateCategory::InstallationWithoutGame,
    ];

    fn candidate(name: &str, category: CandidateCategory) -> SetupCandidate {
        SetupCandidate {
            name: name.to_string(),
            category,
            game: Some(Game::new(name)),
            installation: None,
            snapshot: None,
            missing_dependencies: Vec::new(),
        }
    }

    fn dependency(id: &str) -> GameDependency {
        GameDependency {
            id: id.to_string(),
            source_path: "/tmp/dependency".to_string(),
            source_type: PathContentKind::Directory,
        }
    }

    fn backups(count: usize) -> restic::SnapshotOverview {
        restic::SnapshotOverview {
            snapshot_count: count,
            hosts: BTreeSet::new(),
            latest_snapshot_id: None,
            latest_snapshot_time: None,
            latest_snapshot_host: None,
            unique_paths: Vec::new(),
        }
    }

    fn plain_preview(item: &impl FzfSelectable) -> String {
        let protocol::FzfPreview::Text(text) = item.fzf_preview() else {
            panic!("expected a text preview");
        };
        strip_ansi(&text)
    }

    #[test]
    fn candidate_rows_pair_a_badge_with_the_pending_setup() {
        for category in CATEGORIES {
            let row = candidate("Hades", category).summary_line();
            let hint = candidate("Hades", category).task_hint();
            assert!(
                row.contains(&format_icon_colored(category.icon(), category.color())),
                "{row:?}"
            );
            assert!(row.contains("Hades"), "{row:?}");
            assert!(
                row.contains(&format_with_color(&format!("— {hint}"), category.color())),
                "{row:?}"
            );
        }
    }

    #[test]
    fn hints_count_what_is_pending() {
        let mut dependencies = candidate("Hades", CandidateCategory::MissingDependencies);
        dependencies.missing_dependencies = vec![dependency("dxvk")];
        assert_eq!(dependencies.task_hint(), "Install 1 pending dependency");
        dependencies.missing_dependencies.push(dependency("proton"));
        assert_eq!(dependencies.task_hint(), "Install 2 pending dependencies");

        let mut restored = candidate("Sable", CandidateCategory::SnapshotWithoutGame);
        restored.snapshot = Some(backups(1));
        assert_eq!(restored.task_hint(), "Backups detected (1 snapshot)");
    }

    #[test]
    fn candidate_previews_are_styled_and_list_pending_tasks() {
        let text = plain_preview(&CandidateOption::candidate(candidate(
            "Celeste",
            CandidateCategory::MissingInstallation,
        )));
        assert!(text.contains('─'), "missing the styled header: {text}");
        assert!(text.contains("Pending Tasks"));
        assert!(text.contains("• Save path not configured on this device"));
        assert!(text.contains("Game registered (needs installation on this device)"));
        assert!(text.contains("Press Enter to choose where this game stores its saves."));
        assert!(!text.contains("GAME OVERVIEW"));

        let mut backed_up = candidate("Celeste", CandidateCategory::MissingInstallation);
        backed_up.snapshot = Some(backups(3));
        let text = plain_preview(&CandidateOption::candidate(backed_up));
        assert!(text.contains("Backups"));
        assert!(text.contains("Snapshots:"));
        assert!(text.contains("3"));
        assert!(
            text.contains(
                "Press Enter to choose the save path, then decide between the backup and"
            ),
            "{text}"
        );
    }

    #[test]
    fn previews_show_the_game_this_device_and_its_backups() {
        let mut game = Game::new("Hades");
        game.description = Some("A roguelike".to_string());
        let mut dependencies = candidate("Hades", CandidateCategory::MissingDependencies);
        dependencies.game = Some(game);
        dependencies.missing_dependencies = vec![dependency("dxvk")];
        dependencies.installation = Some(GameInstallation::with_kind(
            "Hades",
            TildePath::new("/tmp/hades".into()),
            PathContentKind::Directory,
        ));
        dependencies.snapshot = Some(backups(2));

        let text = plain_preview(&CandidateOption::candidate(dependencies));
        for expected in [
            "Game",
            "Description:",
            "A roguelike",
            "• Missing dependencies: dxvk (directory)",
            "This Device",
            "Save Path:",
            "Backups",
            "Snapshots: 2",
            "Press Enter to install the missing dependency.",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from:\n{text}"
            );
        }
    }

    #[test]
    fn candidates_are_keyed_by_game_so_the_cursor_can_follow_them() {
        let mut dependencies = candidate("Hades", CandidateCategory::MissingDependencies);
        let key = CandidateOption::candidate(dependencies.clone()).fzf_key();
        dependencies.missing_dependencies = vec![dependency("dxvk")];

        assert_eq!(key, "Hades");
        assert_eq!(CandidateOption::candidate(dependencies).fzf_key(), key);
        assert_eq!(CandidateOption::manual().fzf_key(), "!__manual__");
        assert_eq!(CandidateOption::back().fzf_key(), "!__back__");
    }

    #[test]
    fn fixed_rows_match_the_other_game_menus() {
        let manual = CandidateOption::manual();
        assert!(
            manual
                .fzf_display_text()
                .contains(&format_icon_colored(NerdFont::Edit, colors::BLUE))
        );
        assert!(plain_preview(&manual).contains("Game name"));

        let back = CandidateOption::back();
        assert!(back.fzf_display_text().contains(&format_back_icon()));
        assert!(plain_preview(&back).contains("Leave the setup assistant."));
    }

    #[test]
    fn setup_list_keeps_the_cursor_on_the_game_being_set_up() -> Result<()> {
        let candidates = [
            candidate("Alpha", CandidateCategory::MissingInstallation),
            candidate("Beta", CandidateCategory::MissingInstallation),
            candidate("Gamma", CandidateCategory::SnapshotWithoutGame),
        ];
        let mut cursor = MenuCursor::new();
        let _mock = MockQueue::new().select_index(1).cancel_selection().guard();

        assert!(matches!(
            prompt_installation_choice(&candidates, &mut cursor)?,
            Selection::Candidate(picked) if picked.name == "Beta"
        ));
        assert_eq!(
            cursor.initial_index(&candidate_options(&candidates)),
            Some(1),
            "a cancelled task reopens the list on the same game"
        );
        // Beta is finished, so the game that took its row is focused next.
        let remaining = [candidates[0].clone(), candidates[2].clone()];
        assert_eq!(
            cursor.initial_index(&candidate_options(&remaining)),
            Some(1)
        );

        assert!(matches!(
            prompt_installation_choice(&candidates, &mut cursor)?,
            Selection::Cancelled
        ));
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn task_chooser_returns_the_picked_step_and_nothing_when_dismissed() -> Result<()> {
        let tasks = vec![
            SetupTask::ConfigureDependency {
                id: "dxvk".to_string(),
                source_type: PathContentKind::Directory,
            },
            SetupTask::ConfigureDependency {
                id: "proton".to_string(),
                source_type: PathContentKind::Directory,
            },
        ];
        let option = TaskOption::new("Hades", tasks[1].clone());
        let row = option.fzf_display_text();
        assert!(row.contains(&format_icon_colored(NerdFont::Package, colors::YELLOW)));
        assert!(strip_ansi(&row).contains("Install dependency 'proton'"));
        let text = plain_preview(&option);
        assert!(text.contains('─'));
        assert!(text.contains("Install Dependency"));
        assert!(text.contains("Dependency:"));

        let _mock = MockQueue::new().select_index(1).cancel_selection().guard();
        assert!(matches!(
            prompt_task_choice("Hades", &tasks)?,
            Some(SetupTask::ConfigureDependency { id, .. }) if id == "proton"
        ));
        assert!(prompt_task_choice("Hades", &tasks)?.is_none());
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }
}
