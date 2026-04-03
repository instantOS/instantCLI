use super::discover::MenuSelectionPayload;
use super::manager::GameCreationContext;
use crate::common::TildePath;
use crate::common::shell::resolve_current_binary;
use crate::game::config::PathContentKind;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    DecodedStreamingMenuItem, FilePickerScope, FzfResult, FzfWrapper, Header, PathInputBuilder,
    PathInputSelection, StreamingCommand, StreamingMenuItem,
};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, anyhow};
use std::fs;

#[derive(Debug, Default, Clone)]
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
    SetupTrackedGame {
        game_name: String,
        discovered_save_path: Option<String>,
    },
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
        },
    )
}

pub(super) fn maybe_prefill_from_emulators(
    options: AddGameOptions,
    context: &GameCreationContext,
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
                resolve_discovered_selection(payload, context, options.no_cache)
            }
        },
        FzfResult::Cancelled => Ok(EmulatorPrefillResult::Cancelled),
        _ => Ok(EmulatorPrefillResult::Continue(options)),
    }
}

fn resolve_discovered_selection(
    payload: MenuSelectionPayload,
    context: &GameCreationContext,
    no_cache: bool,
) -> Result<EmulatorPrefillResult> {
    if payload.existing {
        let tracked_name = payload
            .tracked_name
            .or(payload.display_name)
            .unwrap_or_else(|| "Unknown Game".to_string());

        if context.installation_exists(&tracked_name) {
            Ok(EmulatorPrefillResult::OpenGameMenu(tracked_name))
        } else {
            Ok(EmulatorPrefillResult::SetupTrackedGame {
                game_name: tracked_name,
                discovered_save_path: payload.save_path,
            })
        }
    } else {
        Ok(EmulatorPrefillResult::OpenPrefilledAddEditor(
            AddGameOptions {
                name: payload.display_name,
                description: None,
                launch_command: None,
                save_path: payload.save_path,
                create_save_path: false,
                no_cache,
            },
        ))
    }
}

pub(super) fn resolve_add_game_details(
    options: AddGameOptions,
    context: &GameCreationContext,
) -> Result<Option<ResolvedGameDetails>> {
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
        None => match prompt_manual_game_name(context)? {
            Some(name) => name,
            None => return Ok(None),
        },
    };

    let description = match description {
        Some(text) => some_if_not_empty(text),
        None if interactive_prompts => {
            match prompt_optional_text("Enter game description (optional)")? {
                Some(text) => some_if_not_empty(text),
                None => return Ok(None),
            }
        }
        None => None,
    };

    let launch_command = match launch_command {
        Some(command) => some_if_not_empty(command),
        None if interactive_prompts => {
            match prompt_optional_text("Enter launch command (optional)")? {
                Some(command) => some_if_not_empty(command),
                None => return Ok(None),
            }
        }
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
        None => match prompt_manual_save_path(&game_name)? {
            Some(path) => path,
            None => return Ok(None),
        },
    };

    ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory)?;

    let save_path_type = super::relocate::determine_save_path_type(&save_path)?;

    Ok(Some(ResolvedGameDetails {
        name: game_name,
        description,
        launch_command,
        save_path,
        save_path_type,
    }))
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

fn prompt_manual_game_name(context: &GameCreationContext) -> Result<Option<String>> {
    loop {
        let result = FzfWrapper::builder()
            .prompt("Enter game name")
            .input()
            .input_result()?;

        let game_name = match result {
            FzfResult::Selected(name) => name.trim().to_string(),
            FzfResult::Cancelled => return Ok(None),
            _ => return Ok(None),
        };

        if game_name.is_empty() {
            FzfWrapper::message("Game name cannot be empty.")?;
            continue;
        }

        if context.game_exists(&game_name) {
            FzfWrapper::message(&format!("Game '{}' already exists.", game_name))?;
            continue;
        }

        return Ok(Some(game_name));
    }
}

fn prompt_optional_text(prompt: &str) -> Result<Option<String>> {
    match FzfWrapper::builder()
        .prompt(prompt)
        .input()
        .input_result()?
    {
        FzfResult::Selected(value) => Ok(Some(value.trim().to_string())),
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

fn prompt_manual_save_path(game_name: &str) -> Result<Option<TildePath>> {
    let selection = PathInputBuilder::new()
        .header(format!(
            "{} Choose the save path for '{game_name}'",
            char::from(NerdFont::Folder)
        ))
        .manual_prompt(format!(
            "{} Enter the save path (e.g., ~/.local/share/{}/saves)",
            char::from(NerdFont::Edit),
            game_name.to_lowercase().replace(' ', "-")
        ))
        .scope(FilePickerScope::FilesAndDirectories)
        .picker_hint(format!(
            "{} Select the file or directory that stores the save data",
            char::from(NerdFont::Info)
        ))
        .manual_option_label(format!("{} Type an exact path", char::from(NerdFont::Edit)))
        .picker_option_label(format!(
            "{} Browse and choose a path",
            char::from(NerdFont::FolderOpen)
        ))
        .choose()?;

    let save_path = match selection {
        PathInputSelection::Manual(input) => {
            if !super::validation::validate_non_empty(&input, "Save path")? {
                FzfWrapper::message("Save path cannot be empty.")?;
                return prompt_manual_save_path(game_name);
            }
            TildePath::from_str(&input).map_err(|e| anyhow!("Invalid save path: {}", e))?
        }
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
            TildePath::new(path)
        }
        PathInputSelection::Cancelled => return Ok(None),
    };

    if let Err(err) = ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory) {
        FzfWrapper::message(&err.to_string())?;
        return prompt_manual_save_path(game_name);
    }

    let save_path_display = save_path
        .to_tilde_string()
        .unwrap_or_else(|_| save_path.as_path().display().to_string());

    match FzfWrapper::builder()
        .confirm(format!(
            "{} Are you sure you want to use '{save_path_display}' as the save path for '{game_name}'?\n\n\
            This path will be used to store and sync save files for this game.",
            char::from(NerdFont::Question)
        ))
        .yes_text("Use This Path")
        .no_text("Choose Different Path")
        .confirm_dialog()
        .map_err(|e| anyhow!("Failed to get path confirmation: {}", e))?
    {
        crate::menu_utils::ConfirmResult::Yes => {}
        crate::menu_utils::ConfirmResult::No | crate::menu_utils::ConfirmResult::Cancelled => {
            return prompt_manual_save_path(game_name);
        }
    }

    if !save_path.as_path().exists() {
        match FzfWrapper::confirm(&format!(
            "{} Save path '{}' does not exist. Create it?",
            char::from(NerdFont::Warning),
            save_path_display
        ))
        .map_err(|e| anyhow!("Failed to get confirmation: {}", e))?
        {
            crate::menu_utils::ConfirmResult::Yes => {
                fs::create_dir_all(save_path.as_path())
                    .context("Failed to create save directory")?;
                println!(
                    "{} Created save directory: {save_path_display}",
                    char::from(NerdFont::Check)
                );
            }
            crate::menu_utils::ConfirmResult::No | crate::menu_utils::ConfirmResult::Cancelled => {
                return Ok(None);
            }
        }
    }

    Ok(Some(save_path))
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
    use crate::common::TildePath;
    use crate::game::config::{Game, GameInstallation, InstallationsConfig, InstantGameConfig};
    use std::path::PathBuf;

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

    fn make_context(with_installation: bool) -> GameCreationContext {
        let mut installations = Vec::new();
        if with_installation {
            installations.push(GameInstallation::with_kind(
                "Sable",
                TildePath::new(PathBuf::from("/tmp/save")),
                crate::game::config::PathContentKind::Directory,
            ));
        }

        GameCreationContext {
            config: InstantGameConfig {
                repo: TildePath::new(PathBuf::from("/tmp/repo")),
                repo_password: "instantgamepassword".to_string(),
                games: vec![Game::new("Sable")],
                retention_policy: Default::default(),
            },
            installations: InstallationsConfig { installations },
        }
    }

    #[test]
    fn existing_discovered_game_opens_menu_when_installation_exists() {
        let result = resolve_discovered_selection(
            MenuSelectionPayload {
                existing: true,
                display_name: Some("Sable".to_string()),
                tracked_name: Some("Sable".to_string()),
                save_path: Some("/games/Sable".to_string()),
            },
            &make_context(true),
            false,
        )
        .unwrap();

        assert!(matches!(result, EmulatorPrefillResult::OpenGameMenu(name) if name == "Sable"));
    }

    #[test]
    fn existing_discovered_game_without_installation_triggers_setup() {
        let result = resolve_discovered_selection(
            MenuSelectionPayload {
                existing: true,
                display_name: Some("Sable".to_string()),
                tracked_name: Some("Sable".to_string()),
                save_path: Some("/games/Sable".to_string()),
            },
            &make_context(false),
            false,
        )
        .unwrap();

        assert!(matches!(
            result,
            EmulatorPrefillResult::SetupTrackedGame {
                game_name,
                discovered_save_path
            } if game_name == "Sable" && discovered_save_path.as_deref() == Some("/games/Sable")
        ));
    }
}
