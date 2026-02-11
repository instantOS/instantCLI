use super::manager::GameCreationContext;
use super::prompts;
use crate::common::TildePath;
use crate::game::config::PathContentKind;
use crate::game::launch_builder::azahar_discovery;
use crate::game::launch_builder::duckstation_discovery;
use crate::game::launch_builder::eden_discovery;
use crate::game::launch_builder::pcsx2_discovery;
use crate::game::utils::path::tilde_display_string;
use crate::game::utils::safeguards::{ensure_safe_path, PathUsage};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;

/// Options for adding a game non-interactively
#[derive(Debug, Default)]
pub struct AddGameOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub launch_command: Option<String>,
    pub save_path: Option<String>,
    pub create_save_path: bool,
}

/// Result of the emulator discovery pre-fill step
pub(super) enum EmulatorPrefillResult {
    /// Continue with the add-game flow using these options
    Continue(AddGameOptions),
    /// Redirect to the game menu for an already-tracked game
    OpenGameMenu(String),
    /// User cancelled the discovery selection
    Cancelled,
}

pub(super) struct ResolvedGameDetails {
    pub(super) name: String,
    pub(super) description: Option<String>,
    pub(super) launch_command: Option<String>,
    pub(super) save_path: TildePath,
    pub(super) save_path_type: PathContentKind,
}

pub(super) fn maybe_prefill_from_emulators(
    options: AddGameOptions,
    context: &GameCreationContext,
) -> Result<EmulatorPrefillResult> {
    use crate::game::launch_builder::EdenBuilder;

    let eden_installed = eden_discovery::is_eden_installed();
    let pcsx2_installed = pcsx2_discovery::is_pcsx2_installed();
    let duckstation_installed = duckstation_discovery::is_duckstation_installed();
    let azahar_installed = azahar_discovery::is_azahar_installed();

    if !eden_installed && !pcsx2_installed && !duckstation_installed && !azahar_installed {
        return Ok(EmulatorPrefillResult::Continue(options));
    }

    let eden_games = if eden_installed {
        eden_discovery::discover_eden_games()?
    } else {
        Vec::new()
    };

    let pcsx2_memcards = if pcsx2_installed {
        pcsx2_discovery::discover_pcsx2_memcards()?
    } else {
        Vec::new()
    };

    let duckstation_memcards = if duckstation_installed {
        duckstation_discovery::discover_duckstation_memcards()?
    } else {
        Vec::new()
    };

    let azahar_games = if azahar_installed {
        azahar_discovery::discover_azahar_games()?
    } else {
        Vec::new()
    };

    if eden_games.is_empty()
        && pcsx2_memcards.is_empty()
        && duckstation_memcards.is_empty()
        && azahar_games.is_empty()
    {
        return Ok(EmulatorPrefillResult::Continue(options));
    }

    let items = classify_discovered_items(
        &eden_games,
        &pcsx2_memcards,
        &duckstation_memcards,
        &azahar_games,
        context,
    );

    let result = FzfWrapper::builder()
        .header(Header::fancy("Games"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_padded(items)?;

    match result {
        FzfResult::Selected(AddMethodItem::DiscoveredEdenGame(game)) => {
            let launch_command = match game.game_path {
                Some(ref game_file) => EdenBuilder::find_or_select_eden()?
                    .map(|eden_path| EdenBuilder::format_command_simple(&eden_path, game_file)),
                None => None,
            };

            Ok(EmulatorPrefillResult::Continue(AddGameOptions {
                name: Some(game.display_name),
                description: None,
                launch_command,
                save_path: Some(game.save_path.to_string_lossy().to_string()),
                create_save_path: false,
            }))
        }
        FzfResult::Selected(AddMethodItem::Pcsx2Memcard(memcard)) => {
            let launch_command = pcsx2_discovery::get_pcsx2_launch_command(memcard.install_type);

            Ok(EmulatorPrefillResult::Continue(AddGameOptions {
                name: Some(memcard.display_name.clone()),
                description: None,
                launch_command,
                save_path: Some(memcard.memcard_path.to_string_lossy().to_string()),
                create_save_path: false,
            }))
        }
        FzfResult::Selected(AddMethodItem::DuckstationMemcard(memcard)) => {
            let launch_command =
                duckstation_discovery::get_duckstation_launch_command(memcard.install_type);

            Ok(EmulatorPrefillResult::Continue(AddGameOptions {
                name: Some(memcard.display_name.clone()),
                description: None,
                launch_command,
                save_path: Some(memcard.memcard_path.to_string_lossy().to_string()),
                create_save_path: false,
            }))
        }
        FzfResult::Selected(AddMethodItem::AzaharGame(game)) => {
            let launch_command = azahar_discovery::get_azahar_launch_command(game.install_type);

            Ok(EmulatorPrefillResult::Continue(AddGameOptions {
                name: Some(game.display_name.clone()),
                description: None,
                launch_command,
                save_path: Some(game.save_path.to_string_lossy().to_string()),
                create_save_path: false,
            }))
        }
        FzfResult::Selected(AddMethodItem::ExistingEdenGame(info)) => {
            Ok(EmulatorPrefillResult::OpenGameMenu(info.tracked_name))
        }
        FzfResult::Selected(AddMethodItem::ExistingPcsx2Game(info)) => {
            Ok(EmulatorPrefillResult::OpenGameMenu(info.tracked_name))
        }
        FzfResult::Selected(AddMethodItem::ExistingDuckstationGame(info)) => {
            Ok(EmulatorPrefillResult::OpenGameMenu(info.tracked_name))
        }
        FzfResult::Selected(AddMethodItem::ExistingAzaharGame(info)) => {
            Ok(EmulatorPrefillResult::OpenGameMenu(info.tracked_name))
        }
        FzfResult::Selected(AddMethodItem::ManualEntry) => {
            Ok(EmulatorPrefillResult::Continue(options))
        }
        FzfResult::Cancelled => Ok(EmulatorPrefillResult::Cancelled),
        _ => Ok(EmulatorPrefillResult::Continue(options)),
    }
}

pub(super) fn resolve_add_game_details(
    options: AddGameOptions,
    context: &GameCreationContext,
) -> Result<ResolvedGameDetails> {
    let interactive_prompts = options.name.is_none();

    let AddGameOptions {
        name,
        description,
        launch_command,
        save_path,
        create_save_path,
    } = options;

    let game_name = match name {
        Some(raw_name) => {
            let trimmed = raw_name.trim();
            if !super::validation::validate_non_empty(trimmed, "Game name")? {
                return Err(anyhow!("Game name cannot be empty"));
            }

            if context.game_exists(trimmed) {
                return Err(anyhow!("Game '{}' already exists", trimmed));
            }

            trimmed.to_string()
        }
        None => prompts::get_game_name(&context.config)?,
    };

    let description = match description {
        Some(text) => some_if_not_empty(text),
        None if interactive_prompts => some_if_not_empty(prompts::get_game_description()?),
        None => None,
    };

    let launch_command = match launch_command {
        Some(command) => some_if_not_empty(command),
        None if interactive_prompts => some_if_not_empty(prompts::get_launch_command()?),
        None => None,
    };

    let save_path = match save_path {
        Some(path) => {
            let trimmed = path.trim();
            if !super::validation::validate_non_empty(trimmed, "Save path")? {
                return Err(anyhow!("Save path cannot be empty"));
            }

            let tilde_path =
                TildePath::from_str(trimmed).map_err(|e| anyhow!("Invalid save path: {}", e))?;

            ensure_safe_path(tilde_path.as_path(), PathUsage::SaveDirectory)?;

            if !tilde_path.as_path().exists() {
                if create_save_path {
                    fs::create_dir_all(tilde_path.as_path())
                        .context("Failed to create save directory")?;
                    println!(
                        "{} Created save directory: {}",
                        char::from(NerdFont::Check),
                        trimmed
                    );
                } else {
                    return Err(anyhow!(
                        "Save path '{}' does not exist. Use --create-save-path to create it automatically or run '{} game add' without --save-path for interactive setup.",
                        tilde_path.as_path().display(),
                        env!("CARGO_BIN_NAME")
                    ));
                }
            }

            tilde_path
        }
        None => prompts::get_save_path(&game_name)?,
    };

    ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory)?;

    let save_path_type = super::relocate::determine_save_path_type(&save_path)?;

    Ok(ResolvedGameDetails {
        name: game_name,
        description,
        launch_command,
        save_path,
        save_path_type,
    })
}

fn find_existing_game_for_save(save_path: &Path, context: &GameCreationContext) -> Option<String> {
    context
        .installations
        .installations
        .iter()
        .find(|inst| inst.save_path.as_path() == save_path)
        .map(|inst| inst.game_name.0.clone())
}

fn classify_discovered_items(
    eden_games: &[eden_discovery::EdenDiscoveredGame],
    pcsx2_memcards: &[pcsx2_discovery::Pcsx2DiscoveredMemcard],
    duckstation_memcards: &[duckstation_discovery::DuckstationDiscoveredMemcard],
    azahar_games: &[azahar_discovery::AzaharDiscoveredGame],
    context: &GameCreationContext,
) -> Vec<AddMethodItem> {
    let total_count =
        eden_games.len() + pcsx2_memcards.len() + duckstation_memcards.len() + azahar_games.len();
    let mut items: Vec<AddMethodItem> = Vec::with_capacity(total_count + 1);

    items.push(AddMethodItem::ManualEntry);

    for game in eden_games {
        match find_existing_game_for_save(&game.save_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::ExistingEdenGame(ExistingEdenGameInfo {
                    game: game.clone(),
                    tracked_name: existing_name,
                }));
            }
            None => {
                items.push(AddMethodItem::DiscoveredEdenGame(game.clone()));
            }
        }
    }

    for memcard in pcsx2_memcards {
        match find_existing_game_for_save(&memcard.memcard_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::ExistingPcsx2Game(ExistingPcsx2GameInfo {
                    memcard: memcard.clone(),
                    tracked_name: existing_name,
                }));
            }
            None => {
                items.push(AddMethodItem::Pcsx2Memcard(memcard.clone()));
            }
        }
    }

    for memcard in duckstation_memcards {
        match find_existing_game_for_save(&memcard.memcard_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::ExistingDuckstationGame(
                    ExistingDuckstationGameInfo {
                        memcard: memcard.clone(),
                        tracked_name: existing_name,
                    },
                ));
            }
            None => {
                items.push(AddMethodItem::DuckstationMemcard(memcard.clone()));
            }
        }
    }

    for game in azahar_games {
        match find_existing_game_for_save(&game.save_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::ExistingAzaharGame(ExistingAzaharGameInfo {
                    game: game.clone(),
                    tracked_name: existing_name,
                }));
            }
            None => {
                items.push(AddMethodItem::AzaharGame(game.clone()));
            }
        }
    }

    items
}

#[derive(Clone)]
struct ExistingEdenGameInfo {
    game: eden_discovery::EdenDiscoveredGame,
    tracked_name: String,
}

#[derive(Clone)]
struct ExistingPcsx2GameInfo {
    memcard: pcsx2_discovery::Pcsx2DiscoveredMemcard,
    tracked_name: String,
}

#[derive(Clone)]
struct ExistingDuckstationGameInfo {
    memcard: duckstation_discovery::DuckstationDiscoveredMemcard,
    tracked_name: String,
}

#[derive(Clone)]
struct ExistingAzaharGameInfo {
    game: azahar_discovery::AzaharDiscoveredGame,
    tracked_name: String,
}

#[derive(Clone)]
enum AddMethodItem {
    ManualEntry,
    DiscoveredEdenGame(eden_discovery::EdenDiscoveredGame),
    Pcsx2Memcard(pcsx2_discovery::Pcsx2DiscoveredMemcard),
    DuckstationMemcard(duckstation_discovery::DuckstationDiscoveredMemcard),
    AzaharGame(azahar_discovery::AzaharDiscoveredGame),
    ExistingEdenGame(ExistingEdenGameInfo),
    ExistingPcsx2Game(ExistingPcsx2GameInfo),
    ExistingDuckstationGame(ExistingDuckstationGameInfo),
    ExistingAzaharGame(ExistingAzaharGameInfo),
}

impl FzfSelectable for AddMethodItem {
    fn fzf_display_text(&self) -> String {
        match self {
            AddMethodItem::ManualEntry => {
                format!(
                    "{} Enter a new game manually",
                    format_icon_colored(NerdFont::Edit, colors::BLUE)
                )
            }
            AddMethodItem::DiscoveredEdenGame(game) => {
                format!(
                    "{} {} (Switch)",
                    format_icon_colored(NerdFont::Gamepad, colors::GREEN),
                    game.display_name,
                )
            }
            AddMethodItem::Pcsx2Memcard(memcard) => {
                format!(
                    "{} {} (PS2)",
                    format_icon_colored(NerdFont::Disc, colors::SAPPHIRE),
                    memcard.display_name,
                )
            }
            AddMethodItem::DuckstationMemcard(memcard) => {
                format!(
                    "{} {} (PS1)",
                    format_icon_colored(NerdFont::Disc, colors::PEACH),
                    memcard.display_name,
                )
            }
            AddMethodItem::ExistingEdenGame(info) => {
                format!(
                    "{} {} (Switch)",
                    format_icon_colored(NerdFont::Gamepad, colors::MAUVE),
                    info.tracked_name,
                )
            }
            AddMethodItem::ExistingPcsx2Game(info) => {
                format!(
                    "{} {} (PS2)",
                    format_icon_colored(NerdFont::Disc, colors::MAUVE),
                    info.tracked_name,
                )
            }
            AddMethodItem::ExistingDuckstationGame(info) => {
                format!(
                    "{} {} (PS1)",
                    format_icon_colored(NerdFont::Disc, colors::MAUVE),
                    info.tracked_name,
                )
            }
            AddMethodItem::AzaharGame(game) => {
                format!(
                    "{} {} (3DS)",
                    format_icon_colored(NerdFont::Gamepad, colors::YELLOW),
                    game.display_name,
                )
            }
            AddMethodItem::ExistingAzaharGame(info) => {
                format!(
                    "{} {} (3DS)",
                    format_icon_colored(NerdFont::Gamepad, colors::MAUVE),
                    info.tracked_name,
                )
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            AddMethodItem::ManualEntry => "manual".to_string(),
            AddMethodItem::DiscoveredEdenGame(game) => game.title_id.clone(),
            AddMethodItem::Pcsx2Memcard(memcard) => {
                format!("pcsx2-{}", memcard.display_name)
            }
            AddMethodItem::DuckstationMemcard(memcard) => {
                format!("duckstation-{}", memcard.display_name)
            }
            AddMethodItem::AzaharGame(game) => {
                format!("azahar-{}", game.title_id)
            }
            AddMethodItem::ExistingEdenGame(info) => {
                format!("existing-{}", info.game.title_id)
            }
            AddMethodItem::ExistingPcsx2Game(info) => {
                format!("existing-pcsx2-{}", info.memcard.display_name)
            }
            AddMethodItem::ExistingDuckstationGame(info) => {
                format!("existing-duckstation-{}", info.memcard.display_name)
            }
            AddMethodItem::ExistingAzaharGame(info) => {
                format!("existing-azahar-{}", info.game.title_id)
            }
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        match self {
            AddMethodItem::ManualEntry => PreviewBuilder::new()
                .header(NerdFont::Edit, "Manual Entry")
                .text("Enter game details manually.")
                .blank()
                .text("You will be prompted for:")
                .bullet("Game name")
                .bullet("Description (optional)")
                .bullet("Launch command (optional)")
                .bullet("Save data path")
                .build(),
            AddMethodItem::DiscoveredEdenGame(game) => eden_game_preview(game),
            AddMethodItem::Pcsx2Memcard(memcard) => pcsx2_memcard_preview(memcard),
            AddMethodItem::DuckstationMemcard(memcard) => duckstation_memcard_preview(memcard),
            AddMethodItem::AzaharGame(game) => azahar_game_preview(game),
            AddMethodItem::ExistingEdenGame(info) => existing_eden_game_preview(info),
            AddMethodItem::ExistingPcsx2Game(info) => existing_pcsx2_game_preview(info),
            AddMethodItem::ExistingDuckstationGame(info) => existing_duckstation_game_preview(info),
            AddMethodItem::ExistingAzaharGame(info) => existing_azahar_game_preview(info),
        }
    }

    fn fzf_is_selectable(&self) -> bool {
        true
    }
}

fn eden_game_preview(
    game: &eden_discovery::EdenDiscoveredGame,
) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(game.save_path.clone()));

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Gamepad, &game.display_name)
        .text("Platform: Nintendo Switch")
        .text(&format!("Title ID: {}", game.title_id))
        .blank()
        .separator()
        .blank();

    if let Some(ref game_file) = game.game_path {
        builder = builder
            .text("Game file:")
            .bullet(&game_file.to_string_lossy())
            .blank();
    }

    builder
        .text("Save data:")
        .bullet(&save_display)
        .blank()
        .separator()
        .blank()
        .subtext("Auto-discovered from Eden emulator")
        .build()
}

fn pcsx2_memcard_preview(
    memcard: &pcsx2_discovery::Pcsx2DiscoveredMemcard,
) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(memcard.memcard_path.clone()));

    PreviewBuilder::new()
        .header(NerdFont::Disc, &memcard.display_name)
        .text("Platform: PlayStation 2")
        .text(&format!("Source: {}", memcard.install_type))
        .blank()
        .separator()
        .blank()
        .text("Memory card:")
        .bullet(&save_display)
        .blank()
        .separator()
        .blank()
        .subtext("Auto-discovered from PCSX2 emulator")
        .build()
}

fn existing_eden_game_preview(info: &ExistingEdenGameInfo) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(info.game.save_path.clone()));

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Check, &info.tracked_name)
        .text("Platform: Nintendo Switch")
        .text(&format!("Title ID: {}", info.game.title_id))
        .blank()
        .separator()
        .blank()
        .text("Save data:")
        .bullet(&save_display)
        .blank();

    if let Some(ref game_file) = info.game.game_path {
        builder = builder
            .text("Game file:")
            .bullet(&game_file.to_string_lossy())
            .blank();
    }

    builder
        .separator()
        .blank()
        .subtext("Already tracked — press Enter to open game menu")
        .build()
}

fn existing_pcsx2_game_preview(info: &ExistingPcsx2GameInfo) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(info.memcard.memcard_path.clone()));

    PreviewBuilder::new()
        .header(NerdFont::Check, &info.tracked_name)
        .text("Platform: PlayStation 2")
        .text(&format!(
            "Source: {} ({})",
            info.memcard.display_name, info.memcard.install_type
        ))
        .blank()
        .separator()
        .blank()
        .text("Save data:")
        .bullet(&save_display)
        .blank()
        .separator()
        .blank()
        .subtext("Already tracked — press Enter to open game menu")
        .build()
}

fn duckstation_memcard_preview(
    memcard: &duckstation_discovery::DuckstationDiscoveredMemcard,
) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(memcard.memcard_path.clone()));

    PreviewBuilder::new()
        .header(NerdFont::Disc, &memcard.display_name)
        .text("Platform: PlayStation 1")
        .text(&format!("Source: {}", memcard.install_type))
        .blank()
        .separator()
        .blank()
        .text("Memory card:")
        .bullet(&save_display)
        .blank()
        .separator()
        .blank()
        .subtext("Auto-discovered from DuckStation emulator")
        .build()
}

fn existing_duckstation_game_preview(
    info: &ExistingDuckstationGameInfo,
) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(info.memcard.memcard_path.clone()));

    PreviewBuilder::new()
        .header(NerdFont::Check, &info.tracked_name)
        .text("Platform: PlayStation 1")
        .text(&format!(
            "Source: {} ({})",
            info.memcard.display_name, info.memcard.install_type
        ))
        .blank()
        .separator()
        .blank()
        .text("Save data:")
        .bullet(&save_display)
        .blank()
        .separator()
        .blank()
        .subtext("Already tracked — press Enter to open game menu")
        .build()
}

fn azahar_game_preview(
    game: &azahar_discovery::AzaharDiscoveredGame,
) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(game.save_path.clone()));

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Gamepad, &game.display_name)
        .text("Platform: Nintendo 3DS")
        .text(&format!("Title ID: {}", game.title_id))
        .text(&format!("Source: {}", game.install_type))
        .blank()
        .separator()
        .blank();

    if let Some(ref game_file) = game.game_path {
        builder = builder
            .text("Game file:")
            .bullet(&game_file.to_string_lossy())
            .blank();
    }

    builder
        .text("Save data:")
        .bullet(&save_display)
        .blank()
        .separator()
        .blank()
        .subtext("Auto-discovered from Azahar emulator")
        .build()
}

fn existing_azahar_game_preview(
    info: &ExistingAzaharGameInfo,
) -> crate::menu::protocol::FzfPreview {
    let save_display = tilde_display_string(&TildePath::new(info.game.save_path.clone()));

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Check, &info.tracked_name)
        .text("Platform: Nintendo 3DS")
        .text(&format!("Title ID: {}", info.game.title_id))
        .text(&format!("Source: {}", info.game.install_type))
        .blank()
        .separator()
        .blank()
        .text("Save data:")
        .bullet(&save_display)
        .blank();

    if let Some(ref game_file) = info.game.game_path {
        builder = builder
            .text("Game file:")
            .bullet(&game_file.to_string_lossy())
            .blank();
    }

    builder
        .separator()
        .blank()
        .subtext("Already tracked — press Enter to open game menu")
        .build()
}

fn some_if_not_empty(value: impl Into<String>) -> Option<String> {
    let text = value.into();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
