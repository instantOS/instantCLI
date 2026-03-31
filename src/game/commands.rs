use std::ffi::OsString;

use anyhow::Result;

use crate::common::deps::RESTIC;
use crate::common::package::{InstallResult, ensure_all};

use super::cli::{DependencyCommands, GameCommands, GameDiscoverySourceArg};
use super::deps::{
    AddDependencyOptions, InstallDependencyOptions, UninstallDependencyOptions, add_dependency,
    install_dependency, list_dependencies as list_game_dependencies, uninstall_dependency,
};
use super::games::AddGameOptions;
use super::games::{GameManager, remove_game};
use super::games::{discover, display, selection};
use super::menu;
use super::operations::{exec_game_command, launch_game, sync_game_saves};
use super::platforms::discovery::DiscoverySource;
use super::repository::GameRepositoryManager;
use super::repository::manager::InitOptions;
use super::restic::{
    backup_game_saves, handle_restic_command, prune::prune_snapshots, restore_game_saves,
};
use super::setup;
use super::utils::validation::prompt_initialize_if_needed;

#[cfg(debug_assertions)]
use super::cli::DebugCommands;

/// Ensure restic is available, prompting for installation if needed
fn ensure_restic_available() -> Result<()> {
    match ensure_all(&[&RESTIC])? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(()),
        InstallResult::Declined => Err(anyhow::anyhow!("restic installation cancelled")),
        InstallResult::NotAvailable { hint, .. } => {
            Err(anyhow::anyhow!("restic not available: {}", hint))
        }
        InstallResult::Failed { reason } => {
            Err(anyhow::anyhow!("restic installation failed: {}", reason))
        }
    }
}

pub fn handle_game_command(command: GameCommands, debug: bool) -> Result<()> {
    match command {
        GameCommands::Init { repo, password } => {
            ensure_restic_available()?;
            handle_init(debug, repo, password)
        }
        GameCommands::Add {
            name,
            description,
            launch_command,
            save_path,
            create_save_path,
        } => handle_add(AddGameOptions {
            name,
            description,
            launch_command,
            save_path,
            create_save_path,
        }),
        GameCommands::Discover { sources, menu } => handle_discover(menu, &map_sources(&sources)),
        GameCommands::Sync { game_name, force } => {
            ensure_restic_available()?;
            handle_sync(game_name, force)
        }
        GameCommands::Launch { game_name } => handle_launch(game_name),
        GameCommands::Exec { command } => handle_exec(command),
        GameCommands::List => handle_list(),
        GameCommands::Info { game_name } => handle_info(game_name),
        GameCommands::Menu { game_name, gui } => {
            if gui {
                let extra: Vec<String> = game_name.iter().map(|n| n.to_string()).collect();
                return crate::common::terminal::launch_menu_in_terminal(
                    "game",
                    "Game Menu",
                    &extra,
                    debug,
                );
            }
            menu::game_menu(game_name)
        }
        GameCommands::Remove { game_name, force } => handle_remove(game_name, force),
        GameCommands::Backup { game_name } => {
            ensure_restic_available()?;
            handle_backup(game_name)
        }
        GameCommands::Prune {
            game_name,
            zero_changes,
        } => {
            ensure_restic_available()?;
            handle_prune(game_name, zero_changes)
        }
        GameCommands::Restic { args } => {
            ensure_restic_available()?;
            handle_restic_command(args)
        }
        GameCommands::Restore {
            game_name,
            snapshot_id,
            force,
        } => {
            ensure_restic_available()?;
            handle_restore(game_name, snapshot_id, force)
        }
        GameCommands::Setup => {
            ensure_restic_available()?;
            handle_setup()
        }
        GameCommands::Relocate { path, game } => handle_relocate(game, path),
        GameCommands::ScanWinePrefix { prefix, list } => handle_scan_wine_prefix(prefix, list),
        GameCommands::Deps { command } => handle_dependency_command(command),
        #[cfg(debug_assertions)]
        GameCommands::Debug { debug_command } => handle_debug(debug_command),
    }
}

fn handle_scan_wine_prefix(prefix: Option<String>, list: bool) -> Result<()> {
    use crate::common::TildePath;
    use crate::game::platforms::ludusavi;
    use crate::game::utils::path::{is_valid_wine_prefix, tilde_display_string};
    use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
    use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
    use crate::ui::nerd_font::NerdFont;
    use crate::ui::preview::PreviewBuilder;
    use std::path::PathBuf;

    // Resolve prefix path
    let prefix_path: PathBuf = match prefix {
        Some(p) => {
            let expanded = shellexpand::full(&p)
                .map_err(|e| anyhow::anyhow!("Failed to expand path '{}': {}", p, e))?
                .into_owned();
            PathBuf::from(expanded)
        }
        None => {
            // Prompt for prefix path
            let path_input = crate::menu_utils::PathInputBuilder::new()
                .header(format!(
                    "{} Choose a Wine prefix to scan",
                    char::from(NerdFont::Wine)
                ))
                .manual_prompt(format!(
                    "{} Enter Wine prefix path (e.g., ~/.wine or ~/Games/prefix):",
                    char::from(NerdFont::Wine)
                ))
                .scope(crate::menu_utils::FilePickerScope::Directories)
                .picker_hint(format!(
                    "{} Choose a directory containing a drive_c folder",
                    char::from(NerdFont::Info)
                ))
                .manual_option_label(format!("{} Type prefix path", char::from(NerdFont::Edit)))
                .picker_option_label(format!(
                    "{} Browse for prefix directory",
                    char::from(NerdFont::FolderOpen)
                ))
                .choose()?;

            let tilde = match crate::game::utils::path::path_selection_to_tilde(path_input)? {
                Some(t) => t,
                None => return Ok(()),
            };

            tilde.as_path().to_path_buf()
        }
    };

    // Validate it's a wine prefix
    if !is_valid_wine_prefix(&prefix_path) {
        println!(
            "{} '{}' is not a valid Wine prefix (missing drive_c directory).",
            char::from(NerdFont::Warning),
            prefix_path.display()
        );
        return Ok(());
    }

    // Show status
    let status = ludusavi::manifest::manifest_status();
    println!(
        "{} Scanning: {} ({})",
        char::from(NerdFont::Search),
        tilde_display_string(&TildePath::new(prefix_path.clone())),
        status
    );

    // Run scan
    let results = ludusavi::scan_wine_prefix(&prefix_path)?;

    if results.is_empty() {
        println!(
            "{} No Ludusavi-compatible saves found in this prefix.",
            char::from(NerdFont::Info)
        );
        return Ok(());
    }

    // List mode: print results
    if list {
        println!("\n{} Discovered saves:\n", char::from(NerdFont::Check));
        for save in &results {
            let tag_str = if !save.tags.is_empty() {
                format!(" [{}]", save.tags.join(", "))
            } else {
                String::new()
            };
            println!(
                "  {} {}{}",
                char::from(NerdFont::File),
                save.game_name,
                tag_str
            );
            println!("    {}", save.save_path);
        }
        println!("\n{} total saves found.", results.len());
        return Ok(());
    }

    // Interactive mode: fzf menu
    #[derive(Clone)]
    struct ScanResultItem {
        game_name: String,
        save_path: String,
        tags: Vec<String>,
    }

    impl FzfSelectable for ScanResultItem {
        fn fzf_display_text(&self) -> String {
            let icon = if self.tags.iter().any(|t| t == "save") {
                format_icon_colored(NerdFont::File, colors::GREEN)
            } else if self.tags.iter().any(|t| t == "config") {
                format_icon_colored(NerdFont::Gear, colors::YELLOW)
            } else {
                format_icon_colored(NerdFont::File, colors::SUBTEXT0)
            };
            format!("{} {}", icon, self.game_name)
        }

        fn fzf_key(&self) -> String {
            format!("{}|{}", self.game_name, self.save_path)
        }

        fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
            let tag_str = if self.tags.is_empty() {
                "none".to_string()
            } else {
                self.tags.join(", ")
            };
            PreviewBuilder::new()
                .header(NerdFont::File, &self.game_name)
                .text(&format!("Save path: {}", self.save_path))
                .text(&format!("Tags: {}", tag_str))
                .blank()
                .subtext("Press Enter to add this game to tracking")
                .build()
        }
    }

    let items: Vec<ScanResultItem> = results
        .into_iter()
        .map(|r| ScanResultItem {
            game_name: r.game_name,
            save_path: r.save_path,
            tags: r.tags,
        })
        .collect();

    let result = FzfWrapper::builder()
        .header(Header::fancy("Discovered Saves"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_padded(items)?;

    match result {
        FzfResult::Selected(item) => {
            // Pre-fill add game with discovered details
            handle_add(AddGameOptions {
                name: Some(item.game_name),
                description: None,
                launch_command: None,
                save_path: Some(item.save_path),
                create_save_path: false,
            })
        }
        FzfResult::Cancelled => Ok(()),
        _ => Ok(()),
    }
}

fn handle_init(debug: bool, repo: Option<String>, password: Option<String>) -> Result<()> {
    GameRepositoryManager::initialize_game_manager(debug, InitOptions { repo, password })
}

fn handle_add(options: AddGameOptions) -> Result<()> {
    // Prompt to initialize if not already initialized
    if !prompt_initialize_if_needed()? {
        return Ok(());
    }
    GameManager::add_game(options)
}

fn handle_sync(game_name: Option<String>, force: bool) -> Result<()> {
    let _summary = sync_game_saves(game_name, force)?;
    Ok(())
}

fn handle_discover(menu: bool, sources: &[DiscoverySource]) -> Result<()> {
    if menu {
        discover::print_streaming_menu_rows(sources)
    } else {
        discover::list_discovered_games(sources)
    }
}

fn map_sources(sources: &[GameDiscoverySourceArg]) -> Vec<DiscoverySource> {
    sources
        .iter()
        .copied()
        .map(|source| match source {
            GameDiscoverySourceArg::Switch => DiscoverySource::Switch,
            GameDiscoverySourceArg::Ps2 => DiscoverySource::Ps2,
            GameDiscoverySourceArg::Ps1 => DiscoverySource::Ps1,
            GameDiscoverySourceArg::ThreeDs => DiscoverySource::ThreeDs,
            GameDiscoverySourceArg::Epic => DiscoverySource::Epic,
            GameDiscoverySourceArg::Steam => DiscoverySource::Steam,
        })
        .collect()
}

fn handle_launch(game_name: Option<String>) -> Result<()> {
    launch_game(game_name)
}

fn handle_exec(command: Vec<OsString>) -> Result<()> {
    exec_game_command(command)
}

fn handle_remove(game_name: Option<String>, force: bool) -> Result<()> {
    remove_game(game_name, force)
}

fn handle_list() -> Result<()> {
    display::list_games()
}

fn handle_info(game_name: Option<String>) -> Result<()> {
    let game_name = match game_name {
        Some(name) => name,
        None => match selection::select_game_interactive(None)? {
            Some(name) => name,
            None => return Ok(()),
        },
    };

    display::show_game_details(&game_name)
}

fn handle_backup(game_name: Option<String>) -> Result<()> {
    backup_game_saves(game_name)
}

fn handle_prune(game_name: Option<String>, zero_changes: bool) -> Result<()> {
    prune_snapshots(game_name, zero_changes)
}

fn handle_restore(
    game_name: Option<String>,
    snapshot_id: Option<String>,
    force: bool,
) -> Result<()> {
    restore_game_saves(game_name, snapshot_id, force)
}

fn handle_setup() -> Result<()> {
    setup::setup_uninstalled_games()
}

fn handle_relocate(game_name: Option<String>, path: Option<String>) -> Result<()> {
    GameManager::relocate_game(game_name, path)
}

fn handle_dependency_command(command: DependencyCommands) -> Result<()> {
    match command {
        DependencyCommands::Add {
            game_name,
            dependency_id,
            path,
        } => add_dependency(AddDependencyOptions {
            game_name,
            dependency_id,
            source_path: path,
        }),
        DependencyCommands::Install {
            game_name,
            dependency_id,
            path,
        } => install_dependency(InstallDependencyOptions {
            game_name,
            dependency_id,
            install_path: path,
        }),
        DependencyCommands::Uninstall {
            game_name,
            dependency_id,
        } => uninstall_dependency(UninstallDependencyOptions {
            game_name,
            dependency_id,
        }),
        DependencyCommands::List { game_name } => list_game_dependencies(game_name),
    }
}

#[cfg(debug_assertions)]
fn handle_debug(debug_command: DebugCommands) -> Result<()> {
    match debug_command {
        DebugCommands::Tags { game_name } => handle_debug_tags(game_name),
    }
}

#[cfg(debug_assertions)]
fn handle_debug_tags(game_name: Option<String>) -> Result<()> {
    use crate::game::config::InstantGameConfig;
    use crate::game::restic::tags;
    use crate::restic::wrapper::ResticWrapper;
    use anyhow::Context;

    let config = InstantGameConfig::load().context("Failed to load game configuration")?;

    let restic = ResticWrapper::new(
        config.repo.as_path().to_string_lossy().to_string(),
        config.repo_password.clone(),
    )
    .context("Failed to initialize restic wrapper")?;

    let snapshots_json = if let Some(game_name) = game_name {
        // Get snapshots for specific game
        restic
            .list_snapshots_filtered(Some(tags::create_game_tags(&game_name)))
            .context("Failed to list snapshots for game")?
    } else {
        // Get all snapshots with instantgame tag
        restic
            .list_snapshots_filtered(Some(vec![tags::INSTANT_GAME_TAG.to_string()]))
            .context("Failed to list snapshots")?
    };

    let snapshots: Vec<crate::restic::wrapper::Snapshot> =
        serde_json::from_str(&snapshots_json).context("Failed to parse snapshot data")?;

    if snapshots.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }

    let debug_output = tags::debug_snapshot_tags(&snapshots);
    print!("{debug_output}");

    Ok(())
}
