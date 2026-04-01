use super::discover::{MenuSelectionPayload, streaming_menu_preview_command};
use super::manager::GameCreationContext;
use super::prompts;
use crate::common::TildePath;
use crate::common::shell::current_exe_command;
use crate::game::config::PathContentKind;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{FzfResult, FzfWrapper, Header};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
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
    let preview = PreviewBuilder::new()
        .header(NerdFont::Edit, "Manual Entry")
        .text("Enter game details manually.")
        .blank()
        .text("You will be prompted for:")
        .bullet("Game name")
        .bullet("Description (optional)")
        .bullet("Launch command (optional)")
        .bullet("Save data path")
        .build();

    let payload = MenuSelectionPayload {
        existing: false,
        display_name: None,
        tracked_name: None,
        save_path: None,
        launch_command: None,
    };

    let payload_json = serde_json::to_vec(&payload)?;
    Ok(format!(
        "{}\t{}\t{}\t{}\t{}",
        "manual",
        "manual",
        "Enter a new game manually",
        general_purpose::STANDARD.encode(match preview {
            crate::menu::protocol::FzfPreview::Text(text) => text.into_bytes(),
            crate::menu::protocol::FzfPreview::Command(command) => command.into_bytes(),
            crate::menu::protocol::FzfPreview::None => Vec::new(),
        }),
        general_purpose::STANDARD.encode(payload_json),
    ))
}

pub(super) fn maybe_prefill_from_emulators(
    options: AddGameOptions,
    _context: &GameCreationContext,
) -> Result<EmulatorPrefillResult> {
    let discover_command = if options.no_cache {
        format!("{} game discover --menu --no-cache", current_exe_command())
    } else {
        format!("{} game discover --menu", current_exe_command())
    };

    let result = FzfWrapper::builder()
        .header(Header::fancy("Games"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .args([
            "--delimiter",
            "\t",
            "--with-nth",
            "3",
            "--preview",
            streaming_menu_preview_command(),
            "--ansi",
        ])
        .select_streaming_prefilled(&discover_command, &manual_menu_row()?)?;

    match result {
        FzfResult::Selected(line) => match parse_discovery_selection(&line)? {
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

fn parse_discovery_selection(line: &str) -> Result<SelectedDiscovery> {
    let mut fields = line.splitn(5, '\t');
    let kind = fields.next().unwrap_or_default();
    let _key = fields.next().unwrap_or_default();
    let _display = fields.next().unwrap_or_default();
    let _preview = fields.next().unwrap_or_default();
    let payload_b64 = fields.next().unwrap_or_default();

    match kind {
        "manual" => Ok(SelectedDiscovery::ManualEntry),
        "discovered" => {
            let payload_json = general_purpose::STANDARD
                .decode(payload_b64)
                .context("Failed to decode discovery payload")?;
            let payload: MenuSelectionPayload = serde_json::from_slice(&payload_json)
                .context("Failed to parse discovery payload")?;
            Ok(SelectedDiscovery::DiscoveredGame(payload))
        }
        other => Err(anyhow!("Unknown discovery selection kind: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manual_selection() {
        let selection =
            parse_discovery_selection("manual\tmanual\tdisplay\tcHJldmlldw==\te30=").unwrap();
        assert!(matches!(selection, SelectedDiscovery::ManualEntry));
    }

    #[test]
    fn parse_discovered_selection_payload() {
        let payload = MenuSelectionPayload {
            existing: false,
            display_name: Some("Sable".to_string()),
            tracked_name: None,
            save_path: Some("/games/Sable".to_string()),
            launch_command: Some("\"/games/Sable/Sable.exe\"".to_string()),
        };
        let payload_b64 = general_purpose::STANDARD.encode(serde_json::to_vec(&payload).unwrap());
        let line = format!("discovered\tsable\tdisplay\tcHJldmlldw==\t{}", payload_b64);

        match parse_discovery_selection(&line).unwrap() {
            SelectedDiscovery::DiscoveredGame(parsed) => {
                assert_eq!(parsed.display_name.as_deref(), Some("Sable"));
                assert_eq!(parsed.save_path.as_deref(), Some("/games/Sable"));
            }
            SelectedDiscovery::ManualEntry => panic!("expected discovered selection"),
        }
    }
}
