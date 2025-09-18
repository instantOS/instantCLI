use anyhow::Result;
use clap::Subcommand;
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
        for item in launch_items {
            println!("{}", item.display_name());
        }
        Ok(0)
    } else {
        // Convert to menu items with metadata for execution
        let menu_items: Vec<SerializableMenuItem> = launch_items
            .into_iter()
            .map(|item| {
                let display_text = item.display_name().to_string();
                // Add metadata to distinguish item types for execution
                let mut metadata = std::collections::HashMap::new();
                match item {
                    LaunchItem::DesktopApp(_) => {
                        metadata.insert("type".to_string(), "desktop".to_string());
                    }
                    LaunchItem::PathExecutable(_) => {
                        metadata.insert("type".to_string(), "path".to_string());
                    }
                }

                SerializableMenuItem {
                    display_text,
                    preview: FzfPreview::None,
                    metadata: Some(metadata),
                }
            })
            .collect();

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
                    let item_name = &selected[0].display_text;
                    let item_type = &selected[0].metadata;

                    // Reconstruct the launch item based on selection
                    let launch_item = match item_type
                        .as_ref()
                        .and_then(|m| m.get("type"))
                        .map(|s| s.as_str())
                    {
                        Some("desktop") => {
                            // Find the desktop app
                            let items = cache.get_launch_items().await?;
                            items.into_iter()
                                .find(|item| {
                                    matches!(item, LaunchItem::DesktopApp(id) if id.starts_with(item_name) || id.strip_suffix(".desktop") == Some(item_name))
                                })
                                .ok_or_else(|| anyhow::anyhow!("Desktop app not found: {}", item_name))?
                        }
                        Some("path") => {
                            // Find the path executable
                            let items = cache.get_launch_items().await?;
                            items
                                .into_iter()
                                .find(|item| match item {
                                    LaunchItem::PathExecutable(name) => {
                                        name.strip_prefix("path:").unwrap_or(name) == item_name
                                            || name == item_name
                                    }
                                    _ => false,
                                })
                                .ok_or_else(|| {
                                    anyhow::anyhow!("Path executable not found: {}", item_name)
                                })?
                        }
                        _ => {
                            // Fallback: try to find by display name
                            let items = cache.get_launch_items().await?;
                            items
                                .into_iter()
                                .find(|item| item.display_name() == item_name)
                                .ok_or_else(|| {
                                    anyhow::anyhow!("Launch item not found: {}", item_name)
                                })?
                        }
                    };

                    // Execute the selected item
                    execute::execute_launch_item(&launch_item).await?;

                    // Record launch in frecency store
                    if let Err(e) = cache.record_launch_item(&launch_item) {
                        eprintln!("Warning: Failed to record launch: {}", e);
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
            println!("Launched: {}", app_name);
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to launch {}: {}", app_name, e);
            Err(anyhow::anyhow!("Failed to launch application: {}", e))
        }
    }
}
