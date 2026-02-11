use super::manager::GameCreationContext;
use super::prompts;
use crate::common::TildePath;
use crate::game::config::PathContentKind;
use crate::game::platforms::discovery::DiscoveredGame;
use crate::game::platforms::discovery::azahar::{self as azahar_discovery, AzaharDiscoveredGame};
use crate::game::platforms::discovery::duckstation::{
    self as duckstation_discovery, DuckstationDiscoveredMemcard,
};
use crate::game::platforms::discovery::eden::{self as eden_discovery, EdenDiscoveredGame};
use crate::game::platforms::discovery::pcsx2::{self as pcsx2_discovery, Pcsx2DiscoveredMemcard};
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::Path;

#[derive(Debug, Default)]
pub struct AddGameOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub launch_command: Option<String>,
    pub save_path: Option<String>,
    pub create_save_path: bool,
}

pub(super) enum EmulatorPrefillResult {
    Continue(AddGameOptions),
    OpenGameMenu(String),
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
        FzfResult::Selected(AddMethodItem::ManualEntry) => {
            Ok(EmulatorPrefillResult::Continue(options))
        }
        FzfResult::Selected(AddMethodItem::DiscoveredGame(game)) => {
            if game.is_existing() {
                let tracked_name = game
                    .tracked_name()
                    .unwrap_or(game.display_name())
                    .to_string();
                Ok(EmulatorPrefillResult::OpenGameMenu(tracked_name))
            } else {
                Ok(EmulatorPrefillResult::Continue(AddGameOptions {
                    name: Some(game.display_name().to_string()),
                    description: None,
                    launch_command: game.build_launch_command(),
                    save_path: Some(game.save_path().to_string_lossy().to_string()),
                    create_save_path: false,
                }))
            }
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
    eden_games: &[EdenDiscoveredGame],
    pcsx2_memcards: &[Pcsx2DiscoveredMemcard],
    duckstation_memcards: &[DuckstationDiscoveredMemcard],
    azahar_games: &[AzaharDiscoveredGame],
    context: &GameCreationContext,
) -> Vec<AddMethodItem> {
    let total_count =
        eden_games.len() + pcsx2_memcards.len() + duckstation_memcards.len() + azahar_games.len();
    let mut items: Vec<AddMethodItem> = Vec::with_capacity(total_count + 1);

    items.push(AddMethodItem::ManualEntry);

    for game in eden_games {
        match find_existing_game_for_save(&game.save_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(
                    EdenDiscoveredGame::existing(game.clone(), existing_name),
                )));
            }
            None => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(game.clone())));
            }
        }
    }

    for memcard in pcsx2_memcards {
        match find_existing_game_for_save(&memcard.memcard_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(
                    Pcsx2DiscoveredMemcard::existing(memcard.clone(), existing_name),
                )));
            }
            None => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(memcard.clone())));
            }
        }
    }

    for memcard in duckstation_memcards {
        match find_existing_game_for_save(&memcard.memcard_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(
                    DuckstationDiscoveredMemcard::existing(memcard.clone(), existing_name),
                )));
            }
            None => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(memcard.clone())));
            }
        }
    }

    for game in azahar_games {
        match find_existing_game_for_save(&game.save_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(
                    AzaharDiscoveredGame::existing(game.clone(), existing_name),
                )));
            }
            None => {
                items.push(AddMethodItem::DiscoveredGame(Box::new(game.clone())));
            }
        }
    }

    items
}

#[derive(Clone)]
enum AddMethodItem {
    ManualEntry,
    DiscoveredGame(Box<dyn DiscoveredGame>),
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
            AddMethodItem::DiscoveredGame(game) => {
                let icon = if game.is_existing() {
                    format_icon_colored(NerdFont::Gamepad, colors::MAUVE)
                } else {
                    match game.platform_short() {
                        "Switch" => format_icon_colored(NerdFont::Gamepad, colors::GREEN),
                        "PS2" | "PS1" => format_icon_colored(NerdFont::Disc, colors::SAPPHIRE),
                        "3DS" => format_icon_colored(NerdFont::Gamepad, colors::YELLOW),
                        _ => format_icon_colored(NerdFont::Gamepad, colors::GREEN),
                    }
                };
                let display_name = game.tracked_name().unwrap_or(game.display_name());
                format!("{} {} ({})", icon, display_name, game.platform_short())
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            AddMethodItem::ManualEntry => "manual".to_string(),
            AddMethodItem::DiscoveredGame(game) => {
                if game.is_existing() {
                    format!("existing-{}", game.unique_key())
                } else {
                    game.unique_key()
                }
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
            AddMethodItem::DiscoveredGame(game) => game.build_preview(),
        }
    }

    fn fzf_is_selectable(&self) -> bool {
        true
    }
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
