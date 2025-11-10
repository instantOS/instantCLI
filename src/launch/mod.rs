use anyhow::Result;
use clap::Subcommand;
use std::collections::HashMap;
use std::process::Command;

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

    // Get launch items (desktop apps + PATH executables)
    let launch_items = cache.get_launch_items().await?;

    if list_only {
        // Print launch items instead of showing menu
        for item in &launch_items {
            println!("{}", item.display_name());
        }
        Ok(0)
    } else {
        let mut item_lookup: HashMap<String, LaunchItem> =
            HashMap::with_capacity(launch_items.len());

        // Convert to menu items with metadata for execution
        let mut menu_items = Vec::with_capacity(launch_items.len());

        for item in &launch_items {
            let key = item.metadata_key();
            item_lookup.insert(key.clone(), item.clone());

            let mut metadata = HashMap::new();
            metadata.insert("type".to_string(), item.metadata_type().to_string());
            metadata.insert("key".to_string(), key);

            menu_items.push(SerializableMenuItem {
                display_text: item.display_name().to_string(),
                preview: FzfPreview::None,
                metadata: Some(metadata),
            });
        }

        // Use GUI menu to select application
        let client = client::MenuClient::new();

        // Show the scratchpad first for immediate feedback
        client.show()?;

        // Ensure server is running
        client.ensure_server_running()?;

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

                    let key = selected_metadata
                        .get("key")
                        .ok_or_else(|| anyhow::anyhow!("Selection key missing"))?;

                    let launch_item = item_lookup
                        .get(key)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("Launch item not found for key: {}", key))?;

                    // Execute the selected item
                    execute::execute_launch_item(&launch_item).await?;

                    // Record launch in frecency store
                    if let Err(e) = cache.record_launch_item(&launch_item) {
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
}

/// Execute an application by name using PATH resolution
fn execute_application(app_name: &str) -> Result<()> {
    // Use Command::new() with just the executable name (let PATH resolve it)
    let mut cmd = Command::new(app_name);

    // Redirect stdout/stderr to /dev/null for clean background execution
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    // Spawn process in background with detachment
    match cmd.spawn() {
        Ok(_) => {
            println!("Launched: {app_name}");
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to launch {app_name}: {e}");
            Err(anyhow::anyhow!("Failed to launch application: {}", e))
        }
    }
}
