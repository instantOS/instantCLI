use super::manager::GameCreationContext;
use crate::common::TildePath;
use crate::game::config::OwnedSaves;
use crate::game::launch_command::LaunchCommand;
use crate::game::platforms::{LaunchCommandBuilderContext, build_optional_launch_command};
use crate::game::utils::path::prompt_for_save_path;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    FilePickerScope, FzfWrapper, HeaderBuilder, PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result, anyhow};
use std::fs;

#[derive(Debug, Default, Clone)]
pub struct AddGameOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub launch_command: Option<String>,
    pub save_path: Option<String>,
    pub create_save_path: bool,
    pub skip_save_setup: bool,
    pub no_cache: bool,
    pub discovery_launch_hint: Option<crate::game::platforms::discovery::DiscoveryLaunchHint>,
}

pub(super) struct ResolvedGameDetails {
    pub(super) name: String,
    pub(super) description: Option<String>,
    pub(super) launch_command: Option<LaunchCommand>,
    pub(super) saves: Option<OwnedSaves>,
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
        skip_save_setup,
        no_cache: _,
        discovery_launch_hint,
    } = options;
    anyhow::ensure!(
        !skip_save_setup || (save_path.is_none() && !create_save_path),
        "Skipping save setup cannot be combined with a save path or directory creation"
    );

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
        Some(command) => some_if_not_empty(command).map(LaunchCommand::from_shell_or_manual),
        None if interactive_prompts => {
            let save_path_hint = save_path
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(TildePath::from_str);
            let mut builder_context = LaunchCommandBuilderContext::from_game(
                save_path_hint.as_ref().map(TildePath::as_path),
                None,
            );
            builder_context.add_discovery_hint(discovery_launch_hint.as_ref());
            match build_optional_launch_command(Some(&builder_context))? {
                crate::menu_utils::DialogOutcome::Submitted(command) => command,
                crate::menu_utils::DialogOutcome::Cancelled => return Ok(None),
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

            let tilde_path = TildePath::from_str(trimmed);

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

            Some(tilde_path)
        }
        None if skip_save_setup => None,
        None => match super::prompts::get_optional_save_path(&game_name)? {
            crate::menu_utils::DialogOutcome::Submitted(path) => path,
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(None),
        },
    };

    let saves = save_path
        .map(|path| {
            ensure_safe_path(path.as_path(), PathUsage::SaveDirectory)?;
            let kind = super::relocate::determine_save_path_type(&path)?;
            Ok::<_, anyhow::Error>(OwnedSaves::new(path, kind))
        })
        .transpose()?;

    Ok(Some(ResolvedGameDetails {
        name: game_name,
        description,
        launch_command,
        saves,
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
            .input_dialog()?;

        let game_name = match result {
            crate::menu_utils::DialogOutcome::Submitted(name) => name.trim().to_string(),
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(None),
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
        .input_dialog()?
    {
        crate::menu_utils::DialogOutcome::Submitted(value) => Ok(Some(value.trim().to_string())),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
    }
}

pub(super) fn prompt_manual_save_path(game_name: &str) -> Result<Option<TildePath>> {
    prompt_for_save_path(game_name, None, || {
        let selection = PathInputBuilder::new()
            .header(HeaderBuilder::new(
                NerdFont::Folder,
                format!("Choose the save path for '{game_name}'"),
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

        match selection {
            PathInputSelection::Manual(input) => {
                if !super::validation::validate_non_empty(&input, "Save path")? {
                    FzfWrapper::message("Save path cannot be empty.")?;
                    Ok(None)
                } else {
                    Ok(Some(TildePath::from_str(&input)))
                }
            }
            PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
                Ok(Some(TildePath::new(path)))
            }
            PathInputSelection::Cancelled => Ok(None),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::config::{InstallationsConfig, InstantGameConfig};
    use crate::game::launch_command::LaunchCommandKind;
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};

    fn context() -> GameCreationContext {
        GameCreationContext {
            config: InstantGameConfig::default(),
            installations: InstallationsConfig::default(),
        }
    }

    #[test]
    fn manual_add_uses_builder_and_preserves_structured_command_and_wrappers() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let saves = temp.path().join("compatdata/12345/pfx/saves");
        fs::create_dir_all(&saves)?;
        let context = context();
        let _guard = MockQueue::new()
            .input_string("New Game")
            .select_index(3) // Steam
            .input_string("12345")
            .confirm_yes()
            .checklist_confirm(vec![0]) // Gamemode
            .select_index(0) // Configure save backups now
            .select_index(0) // Choose the save path only after building the command
            .input_string(saves.to_string_lossy())
            .confirm_yes()
            .guard();
        let details = resolve_add_game_details(
            AddGameOptions {
                description: Some("Description".into()),
                ..Default::default()
            },
            &context,
        )?
        .context("Game creation was canceled")?;

        assert_eq!(details.name, "New Game");
        assert_eq!(
            details
                .saves
                .as_ref()
                .context("Save setup was skipped")?
                .save_path
                .as_path(),
            saves
        );
        let command = details.launch_command.context("No command was built")?;
        assert!(
            matches!(command.kind, LaunchCommandKind::Steam(ref steam) if steam.app_id == 12345)
        );
        assert!(command.wrappers.gamemode);
        assert_eq!(scripted_responses_remaining(), 0);
        assert!(context.config.games.is_empty());
        assert!(context.installations.installations.is_empty());
        Ok(())
    }

    #[test]
    fn canceling_launcher_selection_does_not_create_a_game_or_save_directory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let saves = temp.path().join("not-created");
        let context = context();
        let _guard = MockQueue::new()
            .input_string("New Game")
            .cancel_selection()
            .guard();
        let details = resolve_add_game_details(
            AddGameOptions {
                description: Some("Description".into()),
                save_path: Some(saves.to_string_lossy().into_owned()),
                create_save_path: true,
                ..Default::default()
            },
            &context,
        )?;
        assert!(details.is_none());
        assert_eq!(scripted_responses_remaining(), 0);
        assert!(context.config.games.is_empty());
        assert!(context.installations.installations.is_empty());
        assert!(!saves.exists());
        Ok(())
    }

    #[test]
    fn named_cli_add_does_not_prompt_for_optional_launch_command() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let _guard = MockQueue::new().cancel_selection().guard();
        let details = resolve_add_game_details(
            AddGameOptions {
                name: Some("CLI Game".into()),
                save_path: Some(temp.path().to_string_lossy().into_owned()),
                ..Default::default()
            },
            &context(),
        )?
        .context("Game creation was canceled")?;
        assert!(details.launch_command.is_none());
        assert_eq!(scripted_responses_remaining(), 1);
        Ok(())
    }

    #[test]
    fn deferring_saves_keeps_the_game_and_launch_command_without_a_binding() -> Result<()> {
        let _guard = MockQueue::new()
            .input_string("New Wine Game")
            .select_index(1) // Set up saves later
            .confirm_yes() // Acknowledge the lack of backups
            .guard();
        let details = resolve_add_game_details(
            AddGameOptions {
                description: Some("New installation".into()),
                launch_command: Some("wine /games/new/game.exe".into()),
                ..Default::default()
            },
            &context(),
        )?
        .context("Game creation was canceled")?;
        assert_eq!(details.name, "New Wine Game");
        assert!(details.saves.is_none());
        assert!(matches!(
            details.launch_command.context("Launcher lost")?.kind,
            LaunchCommandKind::Wine(_)
        ));
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn declining_the_no_backups_warning_returns_to_save_setup() -> Result<()> {
        let _guard = MockQueue::new()
            .input_string("New Game")
            .select_index(1)
            .confirm_no()
            .cancel_selection()
            .guard();
        assert!(
            resolve_add_game_details(
                AddGameOptions {
                    description: Some("Description".into()),
                    launch_command: Some("game".into()),
                    ..Default::default()
                },
                &context()
            )?
            .is_none()
        );
        assert_eq!(scripted_responses_remaining(), 0);
        Ok(())
    }

    #[test]
    fn cli_can_explicitly_defer_saves_without_prompting() -> Result<()> {
        let _guard = MockQueue::new().cancel_selection().guard();
        let details = resolve_add_game_details(
            AddGameOptions {
                name: Some("CLI Game".into()),
                launch_command: Some("game".into()),
                skip_save_setup: true,
                ..Default::default()
            },
            &context(),
        )?
        .context("Game creation was canceled")?;
        assert!(details.saves.is_none());
        assert!(details.launch_command.is_some());
        assert_eq!(scripted_responses_remaining(), 1);
        Ok(())
    }
}
