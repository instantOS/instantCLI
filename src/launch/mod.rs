use anyhow::Result;
use clap::Subcommand;
use std::process::Command;

pub mod cache;

use cache::LaunchCache;
use crate::menu::client;
use crate::menu::protocol::{SerializableMenuItem, FzfPreview};

/// Launch command for application discovery and execution
#[derive(Subcommand, Debug, Clone)]
pub enum LaunchCommands {
    /// Launch application launcher
    #[command(name = "")]
    Launch,
}

/// Handle launch command
pub async fn handle_launch_command() -> Result<i32> {
    // Initialize cache
    let mut cache = LaunchCache::new()?;

    // Get applications (uses cache if fresh, refreshes in background if stale)
    let applications = cache.get_applications().await?;

    // Convert to menu items
    let menu_items: Vec<SerializableMenuItem> = applications
        .into_iter()
        .map(|app| SerializableMenuItem {
            display_text: app,
            preview: FzfPreview::None,
            metadata: None,
        })
        .collect();

    // Use GUI menu to select application
    let client = client::MenuClient::new();

    // Ensure server is running
    client.ensure_server_running()?;

    // Show choice menu
    match client.choice("Launch application:".to_string(), menu_items, false) {
        Ok(selected) => {
            if selected.is_empty() {
                Ok(1) // Cancelled
            } else {
                let app_name = &selected[0].display_text;
                execute_application(app_name)?;

                // Record launch in frecency store
                if let Err(e) = cache.record_launch(app_name) {
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
