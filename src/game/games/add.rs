use super::discover::MenuSelectionPayload;
use super::manager::GameCreationContext;
use super::prompts;
use crate::common::TildePath;
use crate::common::shell::resolve_current_binary;
use crate::game::config::PathContentKind;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    DecodedStreamingMenuItem, FzfResult, FzfWrapper, Header, StreamingCommand, StreamingMenuItem,
};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, anyhow};
use std::fs;

#[derive(Debug, Default)]
pub struct AddGameOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub launch_command: Option<String>,
    pub save_path: Option<String>,
    pub create_save_path: bool,
    pub no_cache: bool,
}

pub(super) enum EmulatorPrefillResult {
    Continue(AddGameOptions),
    OpenGameMenu(String),
    OpenPrefilledAddEditor(AddGameOptions),
    Cancelled,
}

pub(super) struct ResolvedGameDetails {
    pub(super) name: String,
    pub(super) description: Option<String>,
    pub(super) launch_command: Option<String>,
    pub(super) save_path: TildePath,
    pub(super) save_path_type: PathContentKind,
}

fn manual_menu_row() -> Result<String> {
    manual_menu_item()
        .preview(
            PreviewBuilder::new()
                .header(NerdFont::Edit, "Manual Entry")
                .text("Enter game details manually.")
                .blank()
                .text("You will be prompted for:")
                .bullet("Game name")
                .bullet("Description (optional)")
                .bullet("Launch command (optional)")
                .bullet("Save data path")
                .build(),
        )
        .encode()
}

fn manual_menu_item() -> StreamingMenuItem<MenuSelectionPayload> {
    StreamingMenuItem::new(
        "manual",
        "manual",
        format!(
            "{} Enter a new game manually",
            format_icon_colored(NerdFont::Edit, colors::BLUE)
        ),
        MenuSelectionPayload {
            existing: false,
            display_name: None,
            tracked_name: None,
            save_path: None,
            launch_command: None,
        },
    )
}

pub(super) fn maybe_prefill_from_emulators(
    options: AddGameOptions,
    _context: &GameCreationContext,
) -> Result<EmulatorPrefillResult> {
    let mut discover_command = StreamingCommand::new(resolve_current_binary())
        .arg("game")
        .arg("discover")
        .arg("--menu");
    if options.no_cache {
        discover_command = discover_command.arg("--no-cache");
    }

    let result = FzfWrapper::builder()
        .header(Header::fancy("Games"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_encoded_streaming_prefilled::<MenuSelectionPayload, _>(
            discover_command,
            &manual_menu_row()?,
        )?;

    match result {
        FzfResult::Selected(row) => match parse_discovery_selection(row)? {
            SelectedDiscovery::ManualEntry => Ok(EmulatorPrefillResult::Continue(options)),
            SelectedDiscovery::DiscoveredGame(payload) => {
                if payload.existing {
                    let tracked_name = payload
                        .tracked_name
                        .or(payload.display_name)
                        .unwrap_or_else(|| "Unknown Game".to_string());
                    Ok(EmulatorPrefillResult::OpenGameMenu(tracked_name))
                } else {
                    Ok(EmulatorPrefillResult::OpenPrefilledAddEditor(
                        AddGameOptions {
                            name: payload.display_name,
                            description: None,
                            launch_command: payload.launch_command,
                            save_path: payload.save_path,
                            create_save_path: false,
                            no_cache: options.no_cache,
                        },
                    ))
                }
            }
        },
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
        no_cache: _,
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

fn some_if_not_empty(value: impl Into<String>) -> Option<String> {
    let text = value.into();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

enum SelectedDiscovery {
    ManualEntry,
    DiscoveredGame(MenuSelectionPayload),
}

fn parse_discovery_selection(
    row: DecodedStreamingMenuItem<MenuSelectionPayload>,
) -> Result<SelectedDiscovery> {
    match row.kind.as_str() {
        "manual" => Ok(SelectedDiscovery::ManualEntry),
        "discovered" => Ok(SelectedDiscovery::DiscoveredGame(row.payload)),
        other => Err(anyhow!("Unknown discovery selection kind: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manual_selection() {
        let selection = parse_discovery_selection(
            DecodedStreamingMenuItem::<MenuSelectionPayload>::decode(&manual_menu_row().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(selection, SelectedDiscovery::ManualEntry));
    }

    #[test]
    fn parse_discovered_selection_payload() {
        let line = StreamingMenuItem::new(
            "discovered",
            "sable",
            "display",
            MenuSelectionPayload {
                existing: false,
                display_name: Some("Sable".to_string()),
                tracked_name: None,
                save_path: Some("/games/Sable".to_string()),
                launch_command: Some("\"/games/Sable/Sable.exe\"".to_string()),
            },
        )
        .preview(crate::menu::protocol::FzfPreview::Text(
            "preview".to_string(),
        ))
        .encode()
        .unwrap();

        match parse_discovery_selection(
            DecodedStreamingMenuItem::<MenuSelectionPayload>::decode(&line).unwrap(),
        )
        .unwrap()
        {
            SelectedDiscovery::DiscoveredGame(parsed) => {
                assert_eq!(parsed.display_name.as_deref(), Some("Sable"));
                assert_eq!(parsed.save_path.as_deref(), Some("/games/Sable"));
            }
            SelectedDiscovery::ManualEntry => panic!("expected discovered selection"),
        }
    }
}
