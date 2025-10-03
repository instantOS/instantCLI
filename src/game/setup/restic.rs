use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, anyhow};

use crate::game::config::{Game, InstallationsConfig, InstantGameConfig};
use crate::game::games::manager::{AddGameOptions, GameManager};
use crate::game::restic::{cache, tags};
use crate::menu::protocol;
use crate::menu_utils::{FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use super::install;

pub(super) fn maybe_setup_restic_games(
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
        .map_err(|e| anyhow!("Failed to select restic game: {e}"))?;

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

            install::setup_single_game(&candidate.name, game_config, installations)
        }
        ResticCandidateKind::NeedsInstallation | ResticCandidateKind::LocalConfig => {
            install::setup_single_game(&candidate.name, game_config, installations)
        }
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
