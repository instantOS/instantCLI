use super::add::AddGameOptions;
use super::discover::MenuSelectionPayload;
use super::manager::GameCreationContext;
use crate::common::shell::resolve_current_binary;
use crate::menu_utils::{
    DecodedStreamingMenuItem, FilePickerScope, FzfResult, FzfWrapper, Header, PathInputBuilder,
    PathInputSelection, StreamingCommand, StreamingMenuItem,
};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Result, anyhow};

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

fn scan_directory_menu_row() -> Result<String> {
    scan_directory_menu_item()
        .preview(
            PreviewBuilder::new()
                .header(NerdFont::FolderOpen, "Scan Directory")
                .text("Choose a directory or Wine prefix to scan for saves.")
                .blank()
                .text("This runs `ins game discover <path>` and shows only")
                .text("the discovered games from that target location.")
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

fn scan_directory_menu_item() -> StreamingMenuItem<MenuSelectionPayload> {
    StreamingMenuItem::new(
        "scan-directory",
        "scan-directory",
        format!(
            "{} Scan a directory for games",
            format_icon_colored(NerdFont::FolderOpen, colors::TEAL)
        ),
        MenuSelectionPayload {
            existing: false,
            display_name: None,
            tracked_name: None,
            save_path: None,
        },
    )
}

fn back_menu_row() -> Result<String> {
    StreamingMenuItem::new(
        "back",
        "back",
        format!("{} Back", char::from(NerdFont::ArrowLeft)),
        MenuSelectionPayload {
            existing: false,
            display_name: None,
            tracked_name: None,
            save_path: None,
        },
    )
    .preview(
        PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to the previous add-game menu.")
            .build(),
    )
    .encode()
}

pub(super) fn maybe_prefill_from_emulators(
    options: AddGameOptions,
    context: &GameCreationContext,
) -> Result<EmulatorPrefillResult> {
    loop {
        let result = FzfWrapper::builder()
            .header(Header::fancy("Games"))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select_encoded_streaming_prefilled::<MenuSelectionPayload, _>(
                build_discover_command(None, options.no_cache),
                &format!("{}\n{}", scan_directory_menu_row()?, manual_menu_row()?),
            )?;

        match result {
            FzfResult::Selected(row) => match parse_discovery_selection(row)? {
                SelectedDiscovery::ManualEntry => {
                    return Ok(EmulatorPrefillResult::Continue(options));
                }
                SelectedDiscovery::ScanDirectory => {
                    let Some(scan_path) = prompt_scan_directory()? else {
                        continue;
                    };

                    match select_from_scanned_directory(&scan_path, context, options.no_cache)? {
                        DirectoryScanResult::Back => continue,
                        DirectoryScanResult::Resolved(result) => return Ok(result),
                    }
                }
                SelectedDiscovery::DiscoveredGame(payload) => {
                    return resolve_discovered_selection(payload, context, options.no_cache);
                }
                SelectedDiscovery::Back => continue,
            },
            FzfResult::Cancelled => return Ok(EmulatorPrefillResult::Cancelled),
            _ => return Ok(EmulatorPrefillResult::Continue(options)),
        }
    }
}

fn build_discover_command(scan_path: Option<&str>, no_cache: bool) -> StreamingCommand {
    let mut discover_command = StreamingCommand::new(resolve_current_binary())
        .arg("game")
        .arg("discover")
        .arg("--menu");
    if let Some(scan_path) = scan_path {
        discover_command = discover_command.arg(scan_path);
    }
    if no_cache {
        discover_command = discover_command.arg("--no-cache");
    }
    discover_command
}

enum DirectoryScanResult {
    Back,
    Resolved(EmulatorPrefillResult),
}

fn select_from_scanned_directory(
    scan_path: &str,
    context: &GameCreationContext,
    no_cache: bool,
) -> Result<DirectoryScanResult> {
    let result = FzfWrapper::builder()
        .header(Header::fancy("Scanned Games"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_encoded_streaming_prefilled::<MenuSelectionPayload, _>(
            build_discover_command(Some(scan_path), no_cache),
            &back_menu_row()?,
        )?;

    match result {
        FzfResult::Selected(row) => match parse_discovery_selection(row)? {
            SelectedDiscovery::Back => Ok(DirectoryScanResult::Back),
            SelectedDiscovery::DiscoveredGame(payload) => Ok(DirectoryScanResult::Resolved(
                resolve_discovered_selection(payload, context, no_cache)?,
            )),
            SelectedDiscovery::ManualEntry | SelectedDiscovery::ScanDirectory => {
                Ok(DirectoryScanResult::Back)
            }
        },
        FzfResult::Cancelled => Ok(DirectoryScanResult::Back),
        _ => Ok(DirectoryScanResult::Back),
    }
}

fn prompt_scan_directory() -> Result<Option<String>> {
    let selection = PathInputBuilder::new()
        .header(format!(
            "{} Choose a directory or Wine prefix to scan",
            char::from(NerdFont::FolderOpen)
        ))
        .manual_prompt(format!(
            "{} Enter a directory to scan",
            char::from(NerdFont::Edit)
        ))
        .scope(FilePickerScope::Directories)
        .picker_hint(format!(
            "{} Select the directory that contains the game or prefix",
            char::from(NerdFont::Info)
        ))
        .manual_option_label(format!(
            "{} Type a directory path",
            char::from(NerdFont::Edit)
        ))
        .picker_option_label(format!(
            "{} Browse and choose a directory",
            char::from(NerdFont::FolderOpen)
        ))
        .choose()?;

    match selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                FzfWrapper::message("Directory path cannot be empty.")?;
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
            Ok(Some(path.to_string_lossy().to_string()))
        }
        PathInputSelection::Cancelled => Ok(None),
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

enum SelectedDiscovery {
    ManualEntry,
    ScanDirectory,
    Back,
    DiscoveredGame(MenuSelectionPayload),
}

fn parse_discovery_selection(
    row: DecodedStreamingMenuItem<MenuSelectionPayload>,
) -> Result<SelectedDiscovery> {
    match row.kind.as_str() {
        "manual" => Ok(SelectedDiscovery::ManualEntry),
        "scan-directory" => Ok(SelectedDiscovery::ScanDirectory),
        "back" => Ok(SelectedDiscovery::Back),
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
    fn parse_scan_directory_selection() {
        let selection = parse_discovery_selection(
            DecodedStreamingMenuItem::<MenuSelectionPayload>::decode(
                &scan_directory_menu_row().unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(selection, SelectedDiscovery::ScanDirectory));
    }

    #[test]
    fn parse_back_selection() {
        let selection = parse_discovery_selection(
            DecodedStreamingMenuItem::<MenuSelectionPayload>::decode(&back_menu_row().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(selection, SelectedDiscovery::Back));
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
            SelectedDiscovery::ManualEntry
            | SelectedDiscovery::ScanDirectory
            | SelectedDiscovery::Back => panic!("expected discovered selection"),
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
