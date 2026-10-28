use anyhow::{Context, Result};
use std::path::PathBuf;

pub mod desktop;
pub mod discovery;
pub mod execute;
pub mod types;

use crate::menu::protocol::{ChoiceOptions, FzfPreview, SerializableMenuItem};
use crate::menu::{MenuBackend, ResolvedBackend, client, instantmenu};
use crate::menu_utils::{DialogOutcome, FzfWrapper, MenuSelection};
use types::LaunchItem;

/// Get XDG data directories in desktop-entry precedence order.
pub(crate) fn get_xdg_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(home_data) = dirs::data_dir() {
        dirs.push(home_data);
    }

    if let Ok(system_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in system_dirs.split(':') {
            if !dir.is_empty() {
                dirs.push(PathBuf::from(dir));
            }
        }
    } else {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    }

    dirs
}

/// Application discovery and launch options.
#[derive(clap::Args, Debug, Clone)]
pub struct LaunchArgs {
    /// List available applications instead of launching
    #[arg(long)]
    pub list: bool,
    /// Also include executables found in PATH
    #[arg(long, conflicts_with = "terminal")]
    pub include_path: bool,
    /// List only PATH executables and run the selection in the configured terminal
    #[arg(long)]
    pub terminal: bool,
    /// Start the menu with this search query
    #[arg(short = 'q', long)]
    pub query: Option<String>,
    /// Menu backend used by the interactive launcher
    #[arg(short = 'b', long, value_enum, default_value_t = MenuBackend::Auto)]
    pub backend: MenuBackend,
}

impl LaunchArgs {
    fn discover(&self) -> Vec<LaunchItem> {
        if self.terminal {
            discovery::discover_terminal_items()
        } else {
            discovery::discover_launch_items(self.include_path)
        }
    }
    fn choice_options(&self) -> ChoiceOptions {
        ChoiceOptions::new(if self.terminal {
            "Run in terminal:"
        } else {
            "Launch application:"
        })
        .with_frecency_cache(Some(
            if self.terminal {
                "launch-terminal"
            } else {
                "launch"
            }
            .into(),
        ))
        .with_initial_text(self.query.clone())
    }
}

pub async fn handle_launch_command(args: LaunchArgs) -> Result<i32> {
    if args.list {
        let launch_items = tokio::task::spawn_blocking(move || args.discover())
            .await
            .context("application discovery task failed")?;
        handle_list_mode(&launch_items)
    } else {
        handle_interactive_mode(args).await
    }
}

fn handle_list_mode(launch_items: &[LaunchItem]) -> Result<i32> {
    // Print launch items instead of showing menu
    for item in launch_items {
        println!("{}", item);
    }
    Ok(0)
}

async fn handle_interactive_mode(args: LaunchArgs) -> Result<i32> {
    let (sender, receiver) =
        crossbeam_channel::bounded(crate::menu::protocol::STREAM_ITEM_BUFFER_CAPACITY);
    let discovery_args = args.clone();
    tokio::task::spawn_blocking(move || {
        for item in discovery_args.discover() {
            if sender.send(prepare_menu_item(&item)).is_err() {
                break;
            }
        }
    });

    let options = args.choice_options();
    let outcome = match args.backend.resolve(true) {
        ResolvedBackend::Instantmenu => {
            instantmenu::InstantmenuBackend::choice_streaming(&options, receiver)
        }
        ResolvedBackend::Scratchpad => client::HostedMenuClient::new()
            .choice_streaming(options, receiver)
            .map(|outcome| outcome.map(MenuSelection::into_items)),
        ResolvedBackend::Tui => {
            let mut frecency = crate::menu::frecency::MenuFrecency::open(
                options.frecency_cache.as_deref().unwrap_or("launch"),
            )?;
            let ranked = frecency.prepare(receiver.iter().collect());
            let mut builder = FzfWrapper::builder().prompt(&options.prompt);
            if let Some(query) = &options.initial_text {
                builder = builder.query(query);
            }
            let outcome = builder
                .items(ranked)
                .select()
                .map(|outcome| outcome.map(MenuSelection::into_items));
            if let Ok(DialogOutcome::Submitted(ref selected)) = outcome
                && let Err(error) = frecency.record_all(selected)
            {
                eprintln!("Warning: {error:#}");
            }
            outcome
        }
    };

    match outcome {
        Ok(DialogOutcome::Submitted(selected)) => {
            let launch_item = launch_item_from_menu(
                selected
                    .first()
                    .context("Menu submitted an empty selection")?,
            )?;
            execute::execute_launch_item(&launch_item, args.terminal)?;

            Ok(0) // Success
        }
        Ok(crate::menu_utils::DialogOutcome::Cancelled) => Ok(1),
        Err(e) => {
            eprintln!("Error showing menu: {e}");
            Ok(2) // Error
        }
    }
}

fn prepare_menu_item(item: &LaunchItem) -> SerializableMenuItem {
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("type".to_string(), item.metadata_type().to_string());
    metadata.insert("identifier".to_string(), item.identifier().to_string());
    if let LaunchItem::DesktopApp { path, .. } = item {
        metadata.insert("path".to_string(), path.to_string_lossy().into_owned());
    }
    SerializableMenuItem {
        key: Some(item.stable_key()),
        display_text: item.to_string(),
        preview: FzfPreview::None,
        metadata: Some(metadata),
    }
}

fn launch_item_from_menu(item: &SerializableMenuItem) -> Result<LaunchItem> {
    let metadata = item
        .metadata
        .as_ref()
        .context("Selection metadata missing")?;
    let identifier = metadata
        .get("identifier")
        .context("Selection identifier missing")?
        .clone();
    match metadata.get("type").map(String::as_str) {
        Some("desktop") => Ok(LaunchItem::DesktopApp {
            id: identifier,
            name: item.display_text.clone(),
            path: metadata
                .get("path")
                .context("Desktop selection path missing")?
                .into(),
        }),
        Some("path") => Ok(LaunchItem::PathExecutable {
            name: identifier,
            display_name: item.display_text.clone(),
        }),
        _ => anyhow::bail!("Selection type is missing or invalid"),
    }
}
