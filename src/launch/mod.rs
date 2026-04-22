use anyhow::{Context, Result};
use clap::Subcommand;
use std::collections::HashMap;

pub mod cache;
pub mod desktop;
pub mod execute;
pub mod types;

use crate::menu::client;
use crate::menu::protocol::{FzfPreview, SerializableMenuItem};
use cache::LaunchCache;
use types::LaunchItem;

/// Launch command for application discovery and execution
#[derive(Subcommand, Debug, Clone)]
pub enum LaunchCommands {
    /// Launch application launcher
    #[command(name = "")]
    Launch,
}

/// Handle launch command
pub async fn handle_launch_command(list_only: bool) -> Result<i32> {
    // Initialize cache
    let mut cache = LaunchCache::new()?;

    if list_only {
        let launch_items = cache.get_launch_items().await?;
        handle_list_mode(&launch_items)
    } else {
        handle_interactive_mode(&mut cache).await
    }
}

fn handle_list_mode(launch_items: &[LaunchItem]) -> Result<i32> {
    // Print launch items instead of showing menu
    for item in launch_items {
        println!("{}", item);
    }
    Ok(0)
}

async fn handle_interactive_mode(cache: &mut LaunchCache) -> Result<i32> {
    let client = client::MenuClient::new();
    let server_client = client.clone();
    let server_ready = tokio::task::spawn_blocking(move || server_client.ensure_server_running());
    let launch_items = cache.get_launch_items().await?;
    let menu_items = prepare_menu_items(&launch_items);

    server_ready
        .await
        .context("menu server startup task failed")??;

    // Show choice menu
    match client.choice("Launch application:".to_string(), menu_items, false) {
        Ok(selected) => {
            if selected.is_empty() {
                Ok(1) // Cancelled
            } else {
                let selected_metadata = selected[0]
                    .metadata
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Selection metadata missing"))?;

                let index = selected_metadata
                    .get("index")
                    .ok_or_else(|| anyhow::anyhow!("Selection index missing"))?
                    .parse::<usize>()
                    .context("Selection index is invalid")?;

                let launch_item = launch_items
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("Launch item index out of bounds: {index}"))?;

                // Execute the selected item
                execute::execute_launch_item(launch_item).await?;

                // Record launch in frecency store
                if let Err(e) = cache.record_launch_item(launch_item) {
                    eprintln!("Warning: Failed to record launch: {e}");
                }

                Ok(0) // Success
            }
        }
        Err(e) => {
            eprintln!("Error showing menu: {e}");
            Ok(2) // Error
        }
    }
}

fn prepare_menu_items(launch_items: &[LaunchItem]) -> Vec<SerializableMenuItem> {
    let mut menu_items = Vec::with_capacity(launch_items.len());

    for (index, item) in launch_items.iter().enumerate() {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), item.metadata_type().to_string());
        metadata.insert("index".to_string(), index.to_string());

        menu_items.push(SerializableMenuItem {
            key: None,
            display_text: item.to_string(),
            preview: FzfPreview::None,
            metadata: Some(metadata),
        });
    }

    menu_items
}
