use crate::ui::prelude::*;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

use crate::dot::path_serde::TildePath;
use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::game::games::manager::{AddGameOptions, GameManager};
use crate::game::games::validation::validate_game_manager_initialized;
use crate::game::restic::backup::GameBackup;
use crate::game::restic::{cache, tags};
use crate::game::utils::save_files::get_save_directory_info;
use crate::menu::protocol;
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfSelectable, FzfWrapper, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

/// Set up games that have been added but don't have installations configured on this device
pub fn setup_uninstalled_games() -> Result<()> {
    // Load configurations
    let mut game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Check if game manager is initialized
    if !validate_game_manager_initialized()? {
        return Ok(());
    }

    maybe_setup_restic_games(&mut game_config, &mut installations)?;

    // Find games without installations
    let uninstalled_games = find_uninstalled_games(&game_config, &installations)?;

    if uninstalled_games.is_empty() {
        GameManager::add_game(AddGameOptions::default())?;
        return Ok(());
    }

    println!(
        "Found {} game(s) that need to be set up on this device:\n",
        uninstalled_games.len()
    );

    for game_name in &uninstalled_games {
        println!("  • {game_name}");
    }

    println!();

    // Process each uninstalled game
    for game_name in uninstalled_games {
        if let Err(e) = setup_single_game(&game_name, &game_config, &mut installations) {
            emit(
                Level::Error,
                "game.setup.failed",
                &format!(
                    "{} Failed to set up game '{game_name}': {e}",
                    char::from(NerdFont::CrossCircle)
                ),
                None,
            );

            // Ask if user wants to continue with other games
            match FzfWrapper::confirm("Would you like to continue setting up the remaining games?")
                .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => continue,
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Setup cancelled by user.");
                    break;
                }
            }
        }
    }

    Ok(())
}

fn maybe_setup_restic_games(
    game_config: &mut InstantGameConfig,
    installations: &mut InstallationsConfig,
) -> Result<()> {
    loop {
        let candidates = discover_restic_game_candidates(game_config, installations)?;

        if candidates.is_empty() {
            println!(
                "{} No new game setup candidates detected.",
                char::from(NerdFont::Info)
            );
        } else {
            let restic_count = candidates
                .iter()
                .filter(|candidate| candidate.kind != ResticCandidateKind::LocalConfig)
                .count();
            let local_count = candidates
                .iter()
                .filter(|candidate| candidate.kind == ResticCandidateKind::LocalConfig)
                .count();

            println!(
                "\n{} Found {} game setup option{}:",
                char::from(NerdFont::Gamepad),
                candidates.len(),
                if candidates.len() == 1 { "" } else { "s" }
            );

            if restic_count > 0 {
                println!("   • Pick an existing restic backup to bootstrap setup");
            }
            if local_count > 0 {
                println!("   • Configure games already defined in your local configuration");
            }
            println!(
                "   • Enter a different game name\n   • Skip to only configure already-added games"
            );
        }

        let selection = match prompt_restic_game_choice(&candidates)? {
            Some(action) => action,
            None => break,
        };

        match selection {
            ResticSetupAction::Restic(candidate) => {
                handle_restic_candidate(candidate, game_config, installations)?;
                break;
            }
            ResticSetupAction::Manual => {
                GameManager::add_game(AddGameOptions::default())?;
                *game_config =
                    InstantGameConfig::load().context("Failed to reload game configuration")?;
                *installations = InstallationsConfig::load()
                    .context("Failed to reload installations configuration")?;
                break;
            }
            ResticSetupAction::Skip => break,
        }
    }

    Ok(())
}

fn discover_restic_game_candidates(
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
) -> Result<Vec<ResticGameCandidate>> {
    let snapshots =
        cache::get_repository_snapshots(game_config).context("Failed to list restic snapshots")?;

    let mut aggregated: HashMap<String, ResticGameCandidate> = HashMap::new();

    for snapshot in snapshots {
        if let Some(game_name) = tags::extract_game_name_from_tags(&snapshot.tags) {
            let game_entry = game_config
                .games
                .iter()
                .find(|game| game.name.0 == game_name);
            let in_config = game_entry.is_some();
            let has_installation = installations
                .installations
                .iter()
                .any(|inst| inst.game_name.0 == game_name);

            if has_installation {
                continue;
            }

            let kind = if in_config {
                ResticCandidateKind::NeedsInstallation
            } else {
                ResticCandidateKind::NewGame
            };

            let description = game_entry.and_then(|game| game.description.clone());

            let entry = aggregated.entry(game_name.clone()).or_insert_with(|| {
                ResticGameCandidate::new(game_name.clone(), kind, description.clone())
            });

            entry.kind = kind;
            if entry.description.is_none() {
                entry.description = description.clone();
            }

            entry.snapshot_count += 1;
            entry.hosts.insert(snapshot.hostname.clone());

            if entry
                .latest_snapshot_time
                .as_ref()
                .map(|time| snapshot.time > *time)
                .unwrap_or(true)
            {
                entry.latest_snapshot_time = Some(snapshot.time.clone());
                entry.latest_snapshot_host = Some(snapshot.hostname.clone());
            }
        }
    }

    for game in &game_config.games {
        if installations
            .installations
            .iter()
            .any(|inst| inst.game_name.0 == game.name.0)
        {
            continue;
        }

        aggregated
            .entry(game.name.0.clone())
            .and_modify(|entry| {
                if entry.description.is_none() {
                    entry.description = game.description.clone();
                }
            })
            .or_insert_with(|| {
                ResticGameCandidate::new(
                    game.name.0.clone(),
                    ResticCandidateKind::LocalConfig,
                    game.description.clone(),
                )
            });
    }

    let mut candidates: Vec<_> = aggregated.into_values().collect();

    candidates.sort_by(|a, b| {
        b.snapshot_count
            .cmp(&a.snapshot_count)
            .then(b.latest_snapshot_time.cmp(&a.latest_snapshot_time))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(candidates)
}

fn prompt_restic_game_choice(
    candidates: &[ResticGameCandidate],
) -> Result<Option<ResticSetupAction>> {
    let mut options: Vec<ResticSetupOption> = candidates
        .iter()
        .cloned()
        .map(ResticSetupOption::restic)
        .collect();

    options.push(ResticSetupOption::manual());
    options.push(ResticSetupOption::skip());

    let selected = FzfWrapper::select_one(options)
        .map_err(|e| anyhow::anyhow!("Failed to select restic game: {}", e))?;

    Ok(selected.map(|opt| opt.action))
}

fn handle_restic_candidate(
    candidate: ResticGameCandidate,
    game_config: &mut InstantGameConfig,
    installations: &mut InstallationsConfig,
) -> Result<()> {
    println!(
        "\n{} Preparing setup for '{}'...",
        char::from(NerdFont::Gamepad),
        candidate.name
    );

    match candidate.kind {
        ResticCandidateKind::NewGame => {
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

            setup_single_game(&candidate.name, game_config, installations)
        }
        ResticCandidateKind::NeedsInstallation | ResticCandidateKind::LocalConfig => {
            setup_single_game(&candidate.name, game_config, installations)
        }
    }
}

#[derive(Clone, Debug)]
enum ResticSetupAction {
    Restic(ResticGameCandidate),
    Manual,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResticCandidateKind {
    NewGame,
    NeedsInstallation,
    LocalConfig,
}

#[derive(Debug, Clone)]
struct ResticGameCandidate {
    name: String,
    kind: ResticCandidateKind,
    snapshot_count: usize,
    hosts: HashSet<String>,
    latest_snapshot_time: Option<String>,
    latest_snapshot_host: Option<String>,
    description: Option<String>,
}

impl ResticGameCandidate {
    fn new(name: String, kind: ResticCandidateKind, description: Option<String>) -> Self {
        Self {
            name,
            kind,
            snapshot_count: 0,
            hosts: HashSet::new(),
            latest_snapshot_time: None,
            latest_snapshot_host: None,
            description,
        }
    }

    fn display_text(&self) -> String {
        let icon = char::from(NerdFont::Gamepad);
        match self.kind {
            ResticCandidateKind::NewGame => format!("{icon} {} (new)", self.name),
            ResticCandidateKind::NeedsInstallation => {
                format!("{icon} {} (install)", self.name)
            }
            ResticCandidateKind::LocalConfig => format!("{icon} {} (local config)", self.name),
        }
    }

    fn preview(&self) -> protocol::FzfPreview {
        let mut preview = String::new();
        preview.push_str(&format!(
            "{} RESTIC BACKUP SUMMARY\n\n",
            char::from(NerdFont::Gamepad)
        ));
        preview.push_str(&format!("Name: {}\n", self.name));
        preview.push_str(&format!(
            "Status: {}\n",
            match self.kind {
                ResticCandidateKind::NewGame => "Not tracked yet (will be added)",
                ResticCandidateKind::NeedsInstallation => "Tracked game (needs local installation)",
                ResticCandidateKind::LocalConfig => "Tracked locally (no restic backups yet)",
            }
        ));
        preview.push_str(&format!("Snapshots: {}\n", self.snapshot_count));

        if let Some(description) = &self.description {
            if !description.trim().is_empty() {
                preview.push_str(&format!("Description: {}\n", description.trim()));
            }
        }

        if let Some(time) = self
            .latest_snapshot_time
            .as_ref()
            .and_then(|iso| format_snapshot_timestamp(iso, self.latest_snapshot_host.as_deref()))
        {
            preview.push_str(&format!("Last Backup: {}\n", time));
        }

        if !self.hosts.is_empty() {
            let mut hosts: Vec<_> = self.hosts.iter().cloned().collect();
            hosts.sort();
            preview.push_str(&format!("Hosts: {}\n", hosts.join(", ")));
        }

        match self.kind {
            ResticCandidateKind::LocalConfig => {
                preview.push_str(&format!(
                    "\n{} This game is already defined in your local configuration. You'll be prompted to provide a save path manually.\n",
                    char::from(NerdFont::Info)
                ));
            }
            ResticCandidateKind::NeedsInstallation => {
                preview.push_str(&format!(
                    "\n{} Select a save path to configure this installation on the current device.\n",
                    char::from(NerdFont::Info)
                ));
            }
            ResticCandidateKind::NewGame => {
                preview.push_str(&format!(
                    "\n{} A new game entry will be created before configuration.",
                    char::from(NerdFont::Info)
                ));
            }
        }

        protocol::FzfPreview::Text(preview)
    }
}

#[derive(Clone, Debug)]
struct ResticSetupOption {
    action: ResticSetupAction,
}

impl ResticSetupOption {
    fn restic(candidate: ResticGameCandidate) -> Self {
        Self {
            action: ResticSetupAction::Restic(candidate),
        }
    }

    fn manual() -> Self {
        Self {
            action: ResticSetupAction::Manual,
        }
    }

    fn skip() -> Self {
        Self {
            action: ResticSetupAction::Skip,
        }
    }
}

impl FzfSelectable for ResticSetupOption {
    fn fzf_display_text(&self) -> String {
        match &self.action {
            ResticSetupAction::Restic(candidate) => candidate.display_text(),
            ResticSetupAction::Manual => format!(
                "{} Enter a new game name manually",
                char::from(NerdFont::Edit)
            ),
            ResticSetupAction::Skip => format!(
                "{} Skip restic discovery (only configure existing games)",
                char::from(NerdFont::Stop)
            ),
        }
    }

    fn fzf_key(&self) -> String {
        match &self.action {
            ResticSetupAction::Restic(candidate) => format!("restic:{}", candidate.name),
            ResticSetupAction::Manual => "manual".to_string(),
            ResticSetupAction::Skip => "skip".to_string(),
        }
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        match &self.action {
            ResticSetupAction::Restic(candidate) => candidate.preview(),
            ResticSetupAction::Manual => protocol::FzfPreview::Text(format!(
                "{} You'll be prompted for all details, just like '{} game add'.",
                char::from(NerdFont::Edit),
                env!("CARGO_BIN_NAME")
            )),
            ResticSetupAction::Skip => protocol::FzfPreview::Text(
                "Continue without setting up new games from restic right now.".to_string(),
            ),
        }
    }
}

fn format_snapshot_timestamp(iso: &str, host: Option<&str>) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let local = parsed.with_timezone(&chrono::Local);
    let timestamp = local.format("%Y-%m-%d %H:%M:%S").to_string();

    if let Some(host) = host {
        Some(format!("{} ({host})", timestamp))
    } else {
        Some(timestamp)
    }
}

/// Find games that are in the config but don't have installations
fn find_uninstalled_games(
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
) -> Result<Vec<String>> {
    let installed_games: HashSet<_> = installations
        .installations
        .iter()
        .map(|inst| &inst.game_name.0)
        .collect();

    let uninstalled_games: Vec<String> = game_config
        .games
        .iter()
        .filter(|game| !installed_games.contains(&game.name.0))
        .map(|game| game.name.0.clone())
        .collect();

    Ok(uninstalled_games)
}

/// Set up a single game by collecting paths from snapshots and letting user choose
fn setup_single_game(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
) -> Result<()> {
    emit(
        Level::Info,
        "game.setup.start",
        &format!(
            "{} Setting up game: {game_name}",
            char::from(NerdFont::Info)
        ),
        None,
    );

    // Get all snapshots for this game to extract paths
    let snapshots = cache::get_snapshots_for_game(game_name, game_config)
        .context("Failed to get snapshots for game")?;
    let latest_snapshot_id = snapshots.first().map(|snapshot| snapshot.id.clone());

    let mut unique_paths = Vec::new();

    if snapshots.is_empty() {
        emit(
            Level::Warn,
            "game.setup.no_snapshots",
            &format!(
                "{} No snapshots found for game '{game_name}'.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        emit(
            Level::Info,
            "game.setup.hint.add",
            &format!(
                "{} You'll be prompted to choose a save path manually.",
                char::from(NerdFont::Info)
            ),
            None,
        );
    } else {
        unique_paths = extract_unique_paths_from_snapshots(&snapshots)?;

        if unique_paths.is_empty() {
            emit(
                Level::Warn,
                "game.setup.no_paths",
                &format!(
                    "{} No save paths found in snapshots for game '{game_name}'.",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            emit(
                Level::Info,
                "game.setup.hint.manual",
                &format!(
                    "{} You'll be prompted to choose a save path manually.",
                    char::from(NerdFont::Info)
                ),
                None,
            );
        } else {
            println!(
                "\nFound {} unique save path(s) from different devices/snapshots:",
                unique_paths.len()
            );
        }
    }

    // Let user choose from available paths or enter a custom one
    let chosen_path = if unique_paths.is_empty() {
        prompt_manual_save_path(game_name)?
    } else {
        choose_installation_path(game_name, &unique_paths)?
    };

    if let Some(path_str) = chosen_path {
        // Convert to TildePath
        let save_path = TildePath::from_str(&path_str)
            .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?;
        let mut installation = GameInstallation::new(game_name, save_path.clone());

        let mut directory_created = false;
        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "Save path '{path_str}' does not exist. Would you like to create it?"
            ))
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => {
                    std::fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    emit(
                        Level::Success,
                        "game.setup.dir_created",
                        &format!(
                            "{} Created save directory: {path_str}",
                            char::from(NerdFont::Check)
                        ),
                        None,
                    );
                    directory_created = true;
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Directory not created. You can create it later when needed.");
                }
            }
        }

        let save_dir_info = get_save_directory_info(save_path.as_path())
            .with_context(|| format!("Failed to inspect save directory '{path_str}'"))?;
        let path_exists_after = save_path.as_path().exists();
        let mut should_restore =
            path_exists_after && (directory_created || save_dir_info.file_count == 0);

        if path_exists_after && save_dir_info.file_count > 0 {
            let overwrite_prompt = format!(
                "{} The directory '{path_str}' already contains {} file{}.\nRestoring from backup will replace its contents. Proceed?",
                char::from(NerdFont::Warning),
                save_dir_info.file_count,
                if save_dir_info.file_count == 1 {
                    ""
                } else {
                    "s"
                }
            );

            match FzfWrapper::builder()
                .confirm(overwrite_prompt)
                .yes_text("Restore and overwrite")
                .no_text("Choose a different path")
                .show_confirmation()
                .map_err(|e| anyhow::anyhow!("Failed to confirm restore overwrite: {}", e))?
            {
                ConfirmResult::Yes => {
                    should_restore = true;
                }
                ConfirmResult::No => {
                    println!(
                        "{} Keeping existing files in '{path_str}'. Restore skipped.",
                        char::from(NerdFont::Info)
                    );
                    should_restore = false;
                }
                ConfirmResult::Cancelled => {
                    emit(
                        Level::Warn,
                        "game.setup.cancelled",
                        &format!(
                            "{} Setup cancelled for game '{game_name}'.",
                            char::from(NerdFont::Warning)
                        ),
                        None,
                    );
                    return Ok(());
                }
            }
        }

        if should_restore && let Some(snapshot_id) = latest_snapshot_id.as_deref() {
            emit(
                Level::Info,
                "game.setup.restore_latest",
                &format!(
                    "{} Restoring latest backup ({snapshot_id}) into {path_str}...",
                    char::from(NerdFont::Download)
                ),
                None,
            );
            let restore_summary =
                restore_latest_backup(game_name, &save_path, snapshot_id, game_config)?;
            emit(
                Level::Success,
                "game.setup.restore_done",
                &format!("{} {restore_summary}", char::from(NerdFont::Check)),
                None,
            );
            installation.update_checkpoint(snapshot_id.to_string());
        }
        installations.installations.push(installation);
        installations.save()?;

        emit(
            Level::Success,
            "game.setup.success",
            &format!(
                "{} Game '{game_name}' set up successfully with save path: {path_str}",
                char::from(NerdFont::Check)
            ),
            None,
        );
    } else {
        emit(
            Level::Warn,
            "game.setup.cancelled",
            &format!(
                "{} Setup cancelled for game '{game_name}'.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
    }

    println!();
    Ok(())
}

fn restore_latest_backup(
    game_name: &str,
    save_path: &TildePath,
    snapshot_id: &str,
    game_config: &InstantGameConfig,
) -> Result<String> {
    let backup_handler = GameBackup::new(game_config.clone());
    let summary = backup_handler
        .restore_game_backup(game_name, snapshot_id, save_path.as_path())
        .context("Failed to restore latest backup")?;

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(game_name, &repo_path);

    Ok(summary)
}

/// Extract unique paths from all snapshots, grouped by frequency for better presentation
fn extract_unique_paths_from_snapshots(
    snapshots: &[crate::restic::wrapper::Snapshot],
) -> Result<Vec<PathInfo>> {
    let mut path_frequency: HashMap<String, PathInfo> = HashMap::new();

    for snapshot in snapshots {
        for path in &snapshot.paths {
            // Normalize the path to use tilde notation for cross-device compatibility
            let normalized_path = normalize_path_for_cross_device(path);

            let entry = path_frequency
                .entry(normalized_path.clone())
                .or_insert(PathInfo {
                    path: normalized_path,
                    frequency: 0,
                    devices: HashSet::new(),
                    first_seen: snapshot.time.clone(),
                    last_seen: snapshot.time.clone(),
                });

            entry.frequency += 1;
            entry.devices.insert(snapshot.hostname.clone());

            // Update first/last seen (simple string comparison should work for ISO dates)
            if snapshot.time < entry.first_seen {
                entry.first_seen = snapshot.time.clone();
            }
            if snapshot.time > entry.last_seen {
                entry.last_seen = snapshot.time.clone();
            }
        }
    }

    let mut paths: Vec<PathInfo> = path_frequency.into_values().collect();

    // Sort by frequency (most common first) and then by number of devices
    paths.sort_by(|a, b| {
        b.frequency
            .cmp(&a.frequency)
            .then(b.devices.len().cmp(&a.devices.len()))
    });

    Ok(paths)
}

/// Normalize a path for cross-device compatibility by converting /home/<user> to ~
fn normalize_path_for_cross_device(path: &str) -> String {
    // Check if the path starts with /home/
    if let Some(rest) = path.strip_prefix("/home/") {
        // Find the first slash after the username
        if let Some(slash_pos) = rest.find('/') {
            // Extract the part after /home/<username>
            let after_user = &rest[slash_pos..];
            return format!("~{after_user}");
        } else {
            // The path is just /home/<username>, convert to ~
            return "~".to_string();
        }
    }

    // If the path doesn't start with /home/, return it as-is
    path.to_string()
}

/// Information about a path found in snapshots
#[derive(Debug, Clone)]
struct PathInfo {
    path: String,
    frequency: usize,
    devices: HashSet<String>,
    first_seen: String,
    last_seen: String,
}

impl FzfSelectable for PathInfo {
    fn fzf_display_text(&self) -> String {
        let devices_str = if self.devices.len() == 1 {
            format!("1 device: {}", self.devices.iter().next().unwrap())
        } else {
            format!(
                "{} devices: {}",
                self.devices.len(),
                self.devices.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        };

        format!(
            "{} (used {} times on {})",
            self.path, self.frequency, devices_str
        )
    }

    fn fzf_key(&self) -> String {
        self.path.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        let mut preview = String::new();
        preview.push_str(&format!(
            "{} SAVE PATH DETAILS\n\n",
            char::from(NerdFont::Folder)
        ));
        preview.push_str(&format!("Path:           {}\n", self.path));
        preview.push_str(&format!("Usage Count:    {} snapshots\n", self.frequency));
        preview.push_str(&format!(
            "Device Count:   {} unique devices\n",
            self.devices.len()
        ));

        preview.push_str(&format!(
            "\n{}  DEVICES USING THIS PATH:\n",
            char::from(NerdFont::Desktop)
        ));
        for device in &self.devices {
            preview.push_str(&format!("  • {device}\n"));
        }

        // Format dates
        if let (Ok(first), Ok(last)) = (
            chrono::DateTime::parse_from_rfc3339(&self.first_seen),
            chrono::DateTime::parse_from_rfc3339(&self.last_seen),
        ) {
            let first_str = first.format("%Y-%m-%d %H:%M:%S").to_string();
            let last_str = last.format("%Y-%m-%d %H:%M:%S").to_string();

            preview.push_str("\n📅 USAGE TIMELINE:\n");
            preview.push_str(&format!("First Seen:     {first_str}\n"));
            preview.push_str(&format!("Last Seen:      {last_str}\n"));
        }

        protocol::FzfPreview::Text(preview)
    }
}

/// Simple wrapper for string options in fzf
#[derive(Debug, Clone)]
struct StringOption {
    text: String,
    value: String,
}

impl StringOption {
    fn new(text: String, value: String) -> Self {
        Self { text, value }
    }
}

impl FzfSelectable for StringOption {
    fn fzf_display_text(&self) -> String {
        self.text.clone()
    }

    fn fzf_key(&self) -> String {
        self.value.clone()
    }
}

fn tilde_display_string(tilde: &TildePath) -> String {
    tilde
        .to_tilde_string()
        .unwrap_or_else(|_| tilde.as_path().to_string_lossy().to_string())
}

fn prompt_manual_save_path(game_name: &str) -> Result<Option<String>> {
    let prompt = format!(
        "{} Enter the save path for '{}' (e.g., ~/.local/share/{}/saves):",
        char::from(NerdFont::Edit),
        game_name,
        game_name.to_lowercase().replace(' ', "-")
    );

    let path_selection = PathInputBuilder::new()
        .header(format!(
            "{} Choose the save path for '{game_name}'",
            char::from(NerdFont::Folder)
        ))
        .manual_prompt(prompt)
        .scope(FilePickerScope::Directories)
        .picker_hint(format!(
            "{} Select the directory to use for {game_name} save files",
            char::from(NerdFont::Info)
        ))
        .manual_option_label(format!("{} Type an exact path", char::from(NerdFont::Edit)))
        .picker_option_label(format!(
            "{} Browse and choose a folder",
            char::from(NerdFont::FolderOpen)
        ))
        .choose()?;

    match path_selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                println!("Empty path provided. Setup cancelled.");
                Ok(None)
            } else {
                let tilde = TildePath::from_str(trimmed)
                    .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?;
                Ok(Some(tilde_display_string(&tilde)))
            }
        }
        PathInputSelection::Picker(path) => {
            let tilde = TildePath::new(path);
            Ok(Some(tilde_display_string(&tilde)))
        }
        PathInputSelection::Cancelled => {
            println!(
                "{} No path selected. Setup cancelled.",
                char::from(NerdFont::Warning)
            );
            Ok(None)
        }
    }
}

/// Let user choose from available paths or enter a custom one
fn choose_installation_path(game_name: &str, paths: &[PathInfo]) -> Result<Option<String>> {
    //TODO: this should be an fzf wrapper message, as it is followed by a choice
    println!(
        "\n{} Select the save path for '{game_name}':",
        char::from(NerdFont::Folder)
    );

    // Create options including the paths and a custom option
    let mut options = vec![StringOption::new(
        format!("{} Enter a different path", char::from(NerdFont::Edit)),
        "CUSTOM".to_string(),
    )];

    // Add existing paths
    for path_info in paths {
        options.push(StringOption::new(
            path_info.fzf_display_text(),
            path_info.path.clone(),
        ));
    }

    let selected = FzfWrapper::select_one(options)
        .map_err(|e| anyhow::anyhow!("Failed to select path option: {}", e))?;

    match selected {
        Some(selection) => {
            if selection.value == "CUSTOM" {
                prompt_manual_save_path(game_name)
            } else {
                // User selected one of the existing paths
                Ok(Some(selection.value))
            }
        }
        None => {
            println!(
                "{} No path selected. Setup cancelled.",
                char::from(NerdFont::Warning)
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_for_cross_device() {
        // Test /home/<user>/<path> -> ~/<path>
        assert_eq!(
            normalize_path_for_cross_device("/home/alice/.config/game/saves"),
            "~/.config/game/saves"
        );

        assert_eq!(
            normalize_path_for_cross_device("/home/bob/Documents/GameSaves"),
            "~/Documents/GameSaves"
        );

        // Test /home/<user> -> ~
        assert_eq!(normalize_path_for_cross_device("/home/alice"), "~");

        // Test paths that don't start with /home/
        assert_eq!(
            normalize_path_for_cross_device("/opt/game/saves"),
            "/opt/game/saves"
        );

        assert_eq!(
            normalize_path_for_cross_device("~/.local/share/game"),
            "~/.local/share/game"
        );

        // Test root path
        assert_eq!(normalize_path_for_cross_device("/"), "/");

        // Test complex paths with usernames that might contain slashes conceptually
        assert_eq!(
            normalize_path_for_cross_device("/home/user.name/.local/share/Steam/steamapps"),
            "~/.local/share/Steam/steamapps"
        );
    }
}
