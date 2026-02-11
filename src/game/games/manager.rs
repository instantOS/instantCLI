use super::{selection::select_game_interactive, validation::*};
use crate::common::TildePath;
use crate::game::config::{
    Game, GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind,
};
use crate::game::launch_builder::eden_discovery;
use crate::game::launch_builder::pcsx2_discovery;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, Header, PathInputBuilder,
    PathInputSelection,
};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, anyhow};
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

/// Result of the Eden discovery pre-fill step
enum EdenPrefillResult {
    /// Continue with the add-game flow using these options
    Continue(AddGameOptions),
    /// Redirect to the game menu for an already-tracked game
    OpenGameMenu(String),
}

struct GameCreationContext {
    config: InstantGameConfig,
    installations: InstallationsConfig,
}

impl GameCreationContext {
    fn load() -> Result<Self> {
        Ok(Self {
            config: InstantGameConfig::load().context("Failed to load game configuration")?,
            installations: InstallationsConfig::load()
                .context("Failed to load installations configuration")?,
        })
    }

    fn config(&self) -> &InstantGameConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut InstantGameConfig {
        &mut self.config
    }

    fn installations_mut(&mut self) -> &mut InstallationsConfig {
        &mut self.installations
    }

    fn game_exists(&self, name: &str) -> bool {
        self.config.games.iter().any(|g| g.name.0 == name)
    }

    fn save(&self) -> Result<()> {
        self.config.save()?;
        self.installations.save()?;
        Ok(())
    }
}

struct ResolvedGameDetails {
    name: String,
    description: Option<String>,
    launch_command: Option<String>,
    save_path: TildePath,
    save_path_type: PathContentKind,
}

/// Manage game CRUD operations
pub struct GameManager;

impl GameManager {
    /// Add a new game to the configuration
    pub fn add_game(options: AddGameOptions) -> Result<()> {
        let mut context = GameCreationContext::load()?;

        if !validate_game_manager_initialized()? {
            return Ok(());
        }

        // In interactive mode, Eden discovery may redirect to an existing game's menu
        if options.name.is_none() {
            match maybe_prefill_from_eden(options, &context)? {
                EdenPrefillResult::OpenGameMenu(game_name) => {
                    return crate::game::menu::game_menu(Some(game_name));
                }
                EdenPrefillResult::Continue(new_options) => {
                    let details = resolve_add_game_details(new_options, &context, false)?;
                    return Self::finish_add_game(&mut context, details);
                }
            }
        }

        let details = resolve_add_game_details(options, &context, false)?;
        Self::finish_add_game(&mut context, details)
    }

    fn finish_add_game(
        context: &mut GameCreationContext,
        details: ResolvedGameDetails,
    ) -> Result<()> {
        let mut game = Game::new(details.name.clone());
        if let Some(description) = &details.description {
            game.description = Some(description.clone());
        }
        if let Some(command) = &details.launch_command {
            game.launch_command = Some(command.clone());
        }

        context.config_mut().games.push(game);

        context
            .installations_mut()
            .installations
            .push(GameInstallation::with_kind(
                details.name.clone(),
                details.save_path.clone(),
                details.save_path_type,
            ));

        context.save()?;

        let save_path_display = details
            .save_path
            .to_tilde_string()
            .unwrap_or_else(|_| details.save_path.as_path().to_string_lossy().to_string());

        println!("✓ Game '{}' added successfully!", details.name);
        println!(
            "Game configuration saved with save path: {}",
            save_path_display
        );

        Ok(())
    }

    /// Remove a game from the configuration
    pub fn remove_game(game_name: Option<String>, force: bool) -> Result<()> {
        let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;

        let mut installations =
            InstallationsConfig::load().context("Failed to load installations configuration")?;

        let game_name = match game_name {
            Some(name) => name,
            None => match select_game_interactive(Some("Select a game to remove:"))? {
                Some(name) => name,
                None => return Ok(()),
            },
        };

        // Find the game in the configuration
        let game_index = config.games.iter().position(|g| g.name.0 == game_name);

        if game_index.is_none() {
            eprintln!("Game '{game_name}' not found in configuration.");
            return Ok(());
        }

        let game_index = game_index.unwrap();

        if force {
            config.games.remove(game_index);
            config.save()?;

            installations
                .installations
                .retain(|inst| inst.game_name.0 != game_name);
            installations.save()?;

            println!("✓ Game '{game_name}' removed successfully!");
            return Ok(());
        }

        let game = &config.games[game_index];

        // Show game details and ask for confirmation with improved formatting
        match FzfWrapper::builder()
            .confirm(format!(
                "Are you sure you want to remove the following game?\n\n\
                 Game: {}\n\
                 Description: {}\n\
                 Launch command: {}\n\n\
                 This will remove the game from your configuration and save path mapping.",
                game.name.0,
                game.description.as_deref().unwrap_or("None"),
                game.launch_command.as_deref().unwrap_or("None")
            ))
            .yes_text("Remove Game")
            .no_text("Keep Game")
            .show_confirmation()
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
        {
            ConfirmResult::Yes => {
                // Remove the game from the configuration
                config.games.remove(game_index);
                config.save()?;

                // Remove any installations for this game
                installations
                    .installations
                    .retain(|inst| inst.game_name.0 != game_name);
                installations.save()?;

                println!("✓ Game '{game_name}' removed successfully!");
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                println!("Game removal cancelled.");
            }
        }

        Ok(())
    }

    /// Relocate a game's save path to point to a new location (does not move files)
    pub fn relocate_game(game_name: Option<String>, new_path: Option<String>) -> Result<()> {
        let mut context = GameCreationContext::load()?;

        let game_name = match game_name {
            Some(name) => name,
            None => match select_game_interactive(Some("Select a game to move:"))? {
                Some(name) => name,
                None => return Ok(()),
            },
        };

        // Check if game exists in config
        if !context.game_exists(&game_name) {
            eprintln!("Game '{game_name}' not found in configuration.");
            return Ok(());
        }

        // Resolve new path
        let new_path = match new_path {
            Some(path) => {
                let trimmed = path.trim();
                if !validate_non_empty(trimmed, "Save path")? {
                    return Err(anyhow!("Save path cannot be empty"));
                }
                let tilde_path = TildePath::from_str(trimmed)
                    .map_err(|e| anyhow!("Invalid save path: {}", e))?;

                ensure_safe_path(tilde_path.as_path(), PathUsage::SaveDirectory)?;

                if !tilde_path.as_path().exists() {
                    match FzfWrapper::confirm(&format!(
                        "{} Save path '{}' does not exist. Create it?",
                        char::from(NerdFont::Warning),
                        trimmed
                    ))
                    .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
                    {
                        ConfirmResult::Yes => {
                            std::fs::create_dir_all(tilde_path.as_path())
                                .context("Failed to create save directory")?;
                            println!(
                                "{} Created save directory: {}",
                                char::from(NerdFont::Check),
                                trimmed
                            );
                        }
                        ConfirmResult::No | ConfirmResult::Cancelled => {
                            return Err(anyhow!("Save path creation cancelled"));
                        }
                    }
                }

                tilde_path
            }
            None => Self::get_save_path(&game_name)?,
        };

        // Determine path type
        let save_path_type = if new_path.as_path().exists() {
            let metadata = fs::metadata(new_path.as_path()).with_context(|| {
                format!(
                    "Failed to read metadata for save path: {}",
                    new_path.as_path().display()
                )
            })?;
            PathContentKind::from(metadata)
        } else {
            PathContentKind::Directory
        };

        // Update or create installation
        let installations = &mut context.installations_mut().installations;
        if let Some(inst) = installations
            .iter_mut()
            .find(|i| i.game_name.0 == game_name)
        {
            inst.save_path = new_path.clone();
            inst.save_path_type = save_path_type;
            inst.nearest_checkpoint = None;
        } else {
            installations.push(GameInstallation::with_kind(
                game_name.clone(),
                new_path.clone(),
                save_path_type,
            ));
        }

        context.save()?;

        let path_display = new_path
            .to_tilde_string()
            .unwrap_or_else(|_| new_path.as_path().to_string_lossy().to_string());

        println!("✓ Save path for '{game_name}' relocated successfully!");
        println!("New save path: {}", path_display);

        Ok(())
    }

    /// Get and validate game name from user input
    fn get_game_name(config: &InstantGameConfig) -> Result<String> {
        let game_name = FzfWrapper::input("Enter game name")
            .map_err(|e| anyhow::anyhow!("Failed to get game name input: {}", e))?
            .trim()
            .to_string();

        if !validate_non_empty(&game_name, "Game name")? {
            return Err(anyhow::anyhow!("Game name cannot be empty"));
        }

        // Check if game already exists
        if config.games.iter().any(|g| g.name.0 == game_name) {
            eprintln!("Game '{game_name}' already exists!");
            return Err(anyhow::anyhow!("Game already exists"));
        }

        Ok(game_name)
    }

    /// Get optional game description from user input
    pub(crate) fn get_game_description() -> Result<String> {
        Ok(FzfWrapper::input("Enter game description (optional)")
            .map_err(|e| anyhow::anyhow!("Failed to get description input: {}", e))?
            .trim()
            .to_string())
    }

    /// Get optional launch command from user input
    pub(crate) fn get_launch_command() -> Result<String> {
        Ok(FzfWrapper::input("Enter launch command (optional)")
            .map_err(|e| anyhow::anyhow!("Failed to get launch command input: {}", e))?
            .trim()
            .to_string())
    }

    /// Get save path from user input with validation
    fn get_save_path(game_name: &str) -> Result<TildePath> {
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
                if !validate_non_empty(&input, "Save path")? {
                    return Err(anyhow::anyhow!("Save path cannot be empty"));
                }
                TildePath::from_str(&input)
                    .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?
            }
            PathInputSelection::Picker(path) => TildePath::new(path),
            PathInputSelection::WinePrefix(path) => TildePath::new(path),
            PathInputSelection::Cancelled => {
                println!("Game addition cancelled: save path not provided.");
                return Err(anyhow::anyhow!("Save path selection cancelled"));
            }
        };

        if let Err(err) = ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory) {
            println!("{} {}", char::from(NerdFont::CrossCircle), err);
            return Self::get_save_path(game_name);
        }

        let save_path_display = save_path
            .to_tilde_string()
            .unwrap_or_else(|_| save_path.as_path().to_string_lossy().to_string());

        // Confirm the selected save path with the user
        match FzfWrapper::builder()
            .confirm(format!(
                "{} Are you sure you want to use '{save_path_display}' as the save path for '{game_name}'?\n\n\
                This path will be used to store and sync save files for this game.",
                char::from(NerdFont::Question)
            ))
            .yes_text("Use This Path")
            .no_text("Choose Different Path")
            .confirm_dialog()
            .map_err(|e| anyhow::anyhow!("Failed to get path confirmation: {}", e))?
        {
            ConfirmResult::Yes => {
                // Continue with the selected path
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                println!("{} Choosing different save path...", char::from(NerdFont::Info));
                return Self::get_save_path(game_name);
            }
        }

        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "{} Save path '{save_path_display}' does not exist. Create it?",
                char::from(NerdFont::Warning)
            ))
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => {
                    std::fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    println!(
                        "{} Created save directory: {save_path_display}",
                        char::from(NerdFont::Check)
                    );
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!(
                        "{} Game addition cancelled: save path does not exist.",
                        char::from(NerdFont::Warning)
                    );
                    return Err(anyhow::anyhow!("Save path does not exist"));
                }
            }
        }

        Ok(save_path)
    }
}

fn resolve_add_game_details(
    options: AddGameOptions,
    context: &GameCreationContext,
    _skip_eden: bool,
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
            if !validate_non_empty(trimmed, "Game name")? {
                return Err(anyhow!("Game name cannot be empty"));
            }

            if context.game_exists(trimmed) {
                return Err(anyhow!("Game '{}' already exists", trimmed));
            }

            trimmed.to_string()
        }
        None => GameManager::get_game_name(context.config())?,
    };

    let description = match description {
        Some(text) => some_if_not_empty(text),
        None if interactive_prompts => some_if_not_empty(GameManager::get_game_description()?),
        None => None,
    };

    let launch_command = match launch_command {
        Some(command) => some_if_not_empty(command),
        None if interactive_prompts => some_if_not_empty(GameManager::get_launch_command()?),
        None => None,
    };

    let save_path = match save_path {
        Some(path) => {
            let trimmed = path.trim();
            if !validate_non_empty(trimmed, "Save path")? {
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
        None => GameManager::get_save_path(&game_name)?,
    };

    ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory)?;

    let save_path_type = if save_path.as_path().exists() {
        let metadata = fs::metadata(save_path.as_path()).with_context(|| {
            format!(
                "Failed to read metadata for save path: {}",
                save_path.as_path().display()
            )
        })?;
        PathContentKind::from(metadata)
    } else {
        PathContentKind::Directory
    };

    Ok(ResolvedGameDetails {
        name: game_name,
        description,
        launch_command,
        save_path,
        save_path_type,
    })
}

/// Offer Eden game discovery as an alternative to manual entry.
///
/// If Eden is installed and games with saves are found, presents a selection
/// menu. If the user picks a discovered game, the options are prepopulated
/// with name, launch command, and save path from the discovered data.
fn maybe_prefill_from_eden(
    options: AddGameOptions,
    context: &GameCreationContext,
) -> Result<EdenPrefillResult> {
    use crate::game::launch_builder::EdenBuilder;

    // Check if any emulator is installed with discoverable saves
    let eden_installed = eden_discovery::is_eden_installed();
    let pcsx2_installed = pcsx2_discovery::is_pcsx2_installed();

    if !eden_installed && !pcsx2_installed {
        return Ok(EdenPrefillResult::Continue(options));
    }

    // Discover Eden games
    let eden_games = if eden_installed {
        eden_discovery::discover_eden_games()?
    } else {
        Vec::new()
    };

    // Discover PCSX2 memcards
    let pcsx2_memcards = if pcsx2_installed {
        pcsx2_discovery::discover_pcsx2_memcards()?
    } else {
        Vec::new()
    };

    // If nothing discovered, continue with manual entry
    if eden_games.is_empty() && pcsx2_memcards.is_empty() {
        return Ok(EdenPrefillResult::Continue(options));
    }

    // Classify each discovered item as new or already tracked
    let items = classify_discovered_items(&eden_games, &pcsx2_memcards, context);

    let result = FzfWrapper::builder()
        .header(Header::fancy("Games"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_padded(items)?;

    match result {
        FzfResult::Selected(AddMethodItem::EdenGame(game)) => {
            // Build the launch command if we have a ROM path
            let launch_command = match game.game_path {
                Some(ref game_file) => match EdenBuilder::find_or_select_eden()? {
                    Some(eden_path) => {
                        let eden_str = eden_path.to_string_lossy();
                        let game_str = game_file.to_string_lossy();
                        Some(format!("\"{}\" -g \"{}\"", eden_str, game_str))
                    }
                    None => None,
                },
                None => None,
            };

            Ok(EdenPrefillResult::Continue(AddGameOptions {
                name: Some(game.display_name),
                description: None,
                launch_command,
                save_path: Some(game.save_path.to_string_lossy().to_string()),
                create_save_path: false,
            }))
        }
        FzfResult::Selected(AddMethodItem::Pcsx2Memcard(memcard)) => {
            // Get the appropriate launch command for this PCSX2 installation type
            let launch_command =
                pcsx2_discovery::get_pcsx2_launch_command(memcard.install_type);

            Ok(EdenPrefillResult::Continue(AddGameOptions {
                name: Some(memcard.display_name.clone()),
                description: None,
                launch_command,
                save_path: Some(memcard.memcard_path.to_string_lossy().to_string()),
                create_save_path: false,
            }))
        }
        FzfResult::Selected(AddMethodItem::ExistingGame(info)) => {
            Ok(EdenPrefillResult::OpenGameMenu(info.tracked_name))
        }
        FzfResult::Selected(AddMethodItem::ExistingPcsx2Game(info)) => {
            Ok(EdenPrefillResult::OpenGameMenu(info.tracked_name))
        }
        FzfResult::Selected(AddMethodItem::ManualEntry) => Ok(EdenPrefillResult::Continue(options)),
        _ => Ok(EdenPrefillResult::Continue(options)),
    }
}

/// Check if a discovered save path is already tracked by an existing installation.
/// Returns the game name if found.
fn find_existing_game_for_save(save_path: &Path, context: &GameCreationContext) -> Option<String> {
    context
        .installations
        .installations
        .iter()
        .find(|inst| inst.save_path.as_path() == save_path)
        .map(|inst| inst.game_name.0.clone())
}

/// Build the selection item list, classifying each discovered item as new
/// or already tracked (matched by save path against existing installations).
fn classify_discovered_items(
    eden_games: &[eden_discovery::EdenDiscoveredGame],
    pcsx2_memcards: &[pcsx2_discovery::Pcsx2DiscoveredMemcard],
    context: &GameCreationContext,
) -> Vec<AddMethodItem> {
    let total_count = eden_games.len() + pcsx2_memcards.len();
    let mut items: Vec<AddMethodItem> = Vec::with_capacity(total_count + 1);

    items.push(AddMethodItem::ManualEntry);

    // Add Eden games
    for game in eden_games {
        match find_existing_game_for_save(&game.save_path, context) {
            Some(existing_name) => {
                items.push(AddMethodItem::ExistingGame(ExistingGameInfo {
                    game: game.clone(),
                    tracked_name: existing_name,
                }));
            }
            None => {
                items.push(AddMethodItem::EdenGame(game.clone()));
            }
        }
    }

    // Add PCSX2 memcards
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

    items
}

/// Info about a discovered game that is already tracked
#[derive(Clone)]
struct ExistingGameInfo {
    game: eden_discovery::EdenDiscoveredGame,
    tracked_name: String,
}

/// Info about a discovered PCSX2 memcard that is already tracked
#[derive(Clone)]
struct ExistingPcsx2GameInfo {
    memcard: pcsx2_discovery::Pcsx2DiscoveredMemcard,
    tracked_name: String,
}

/// Selection item for the add-game method chooser
#[derive(Clone)]
enum AddMethodItem {
    ManualEntry,
    EdenGame(eden_discovery::EdenDiscoveredGame),
    Pcsx2Memcard(pcsx2_discovery::Pcsx2DiscoveredMemcard),
    ExistingGame(ExistingGameInfo),
    ExistingPcsx2Game(ExistingPcsx2GameInfo),
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
            AddMethodItem::EdenGame(game) => {
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
            AddMethodItem::ExistingGame(info) => {
                format!(
                    "{} {}",
                    format_icon_colored(NerdFont::Gamepad, colors::MAUVE),
                    info.tracked_name,
                )
            }
            AddMethodItem::ExistingPcsx2Game(info) => {
                format!(
                    "{} {}",
                    format_icon_colored(NerdFont::Disc, colors::MAUVE),
                    info.tracked_name,
                )
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            AddMethodItem::ManualEntry => "manual".to_string(),
            AddMethodItem::EdenGame(game) => game.title_id.clone(),
            AddMethodItem::Pcsx2Memcard(memcard) => {
                format!("pcsx2-{}", memcard.display_name)
            }
            AddMethodItem::ExistingGame(info) => {
                format!("existing-{}", info.game.title_id)
            }
            AddMethodItem::ExistingPcsx2Game(info) => {
                format!("existing-pcsx2-{}", info.memcard.display_name)
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
            AddMethodItem::EdenGame(game) => eden_game_preview(game),
            AddMethodItem::Pcsx2Memcard(memcard) => pcsx2_memcard_preview(memcard),
            AddMethodItem::ExistingGame(info) => {
                let home = home_dir_string();
                let save_display = info.game.save_path.to_string_lossy().replace(&home, "~");

                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::Check, &info.tracked_name)
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
            AddMethodItem::ExistingPcsx2Game(info) => {
                let home = home_dir_string();
                let save_display = info
                    .memcard
                    .memcard_path
                    .to_string_lossy()
                    .replace(&home, "~");

                PreviewBuilder::new()
                    .header(NerdFont::Check, &info.tracked_name)
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
        }
    }

    fn fzf_is_selectable(&self) -> bool {
        true
    }
}

fn eden_game_preview(
    game: &eden_discovery::EdenDiscoveredGame,
) -> crate::menu::protocol::FzfPreview {
    let home = home_dir_string();
    let save_display = game.save_path.to_string_lossy().replace(&home, "~");

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Gamepad, &game.display_name)
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
    let home = home_dir_string();
    let save_display = memcard.memcard_path.to_string_lossy().replace(&home, "~");

    PreviewBuilder::new()
        .header(NerdFont::Disc, &memcard.display_name)
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

fn home_dir_string() -> String {
    dirs::home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default()
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
