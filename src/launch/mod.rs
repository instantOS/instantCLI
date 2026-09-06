use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

pub mod desktop;
pub mod discovery;
pub mod execute;
pub mod types;

use crate::menu::protocol::{FzfPreview, SerializableMenuItem};
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

/// Launch command for application discovery and execution
#[derive(Subcommand, Debug, Clone)]
pub enum LaunchCommands {
    /// Launch application launcher
    #[command(name = "")]
    Launch,
}

/// Handle launch command
pub async fn handle_launch_command(list_only: bool, backend: MenuBackend) -> Result<i32> {
    if list_only {
        let launch_items = tokio::task::spawn_blocking(discovery::discover_launch_items)
            .await
            .context("application discovery task failed")?;
        handle_list_mode(&launch_items)
    } else {
        handle_interactive_mode(backend).await
    }
}

fn handle_list_mode(launch_items: &[LaunchItem]) -> Result<i32> {
    // Print launch items instead of showing menu
    for item in launch_items {
        println!("{}", item);
    }
    Ok(0)
}

async fn handle_interactive_mode(backend: MenuBackend) -> Result<i32> {
    let (sender, receiver) =
        crossbeam_channel::bounded(crate::menu::protocol::STREAM_ITEM_BUFFER_CAPACITY);
    tokio::task::spawn_blocking(move || {
        for item in discovery::discover_launch_items() {
            if sender.send(prepare_menu_item(&item)).is_err() {
                break;
            }
        }
    });

    let outcome = match backend.resolve(true) {
        ResolvedBackend::Instantmenu => instantmenu::InstantmenuBackend::choice_streaming(
            "Launch application:",
            receiver,
            false,
            Some("launch"),
        ),
        ResolvedBackend::Scratchpad => client::HostedMenuClient::new()
            .choice_streaming(
                crate::menu::protocol::ChoiceOptions::new("Launch application:")
                    .with_frecency_cache(Some("launch".to_string())),
                receiver,
            )
            .map(|outcome| outcome.map(MenuSelection::into_items)),
        ResolvedBackend::Tui => {
            let mut frecency = crate::menu::frecency::MenuFrecency::open("launch")?;
            let ranked = frecency.prepare(receiver.iter().collect());
            let outcome = FzfWrapper::builder()
                .prompt("Launch application:")
                .select(ranked)
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
            execute::execute_launch_item(&launch_item)?;

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
