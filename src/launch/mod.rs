use anyhow::Result;
use clap::Subcommand;
use std::process::Command;

pub mod cache;
pub mod desktop;
pub mod types;

use crate::launch::types::LaunchItem;
use crate::menu::client;
use crate::menu::protocol::{FzfPreview, SerializableMenuItem};
use cache::LaunchCache;

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

    // Get launch items (uses cache if fresh, refreshes in background if stale)
    let launch_items = cache.get_launch_items().await?;

    // Get display names with conflict resolution
    let display_names = cache.get_display_names().await?;

    // If list only, just print the items and exit
    if list_only {
        for name in display_names {
            println!("{}", name);
        }
        return Ok(0);
    }

    // Convert to menu items
    let menu_items: Vec<SerializableMenuItem> = display_names
        .into_iter()
        .map(|name| SerializableMenuItem {
            display_text: name,
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
                let display_name = &selected[0].display_text;
                execute_launch_item(display_name, &launch_items)?;

                // Record launch in frecency store (use base name without path: prefix)
                let record_name = if display_name.starts_with("path:") {
                    &display_name[5..]
                } else {
                    display_name
                };

                if let Err(e) = cache.record_launch(record_name) {
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

/// Execute a launch item by display name
fn execute_launch_item(
    display_name: &str,
    launch_items: &[crate::launch::types::LaunchItemWithMetadata],
) -> Result<()> {
    // Find the corresponding launch item
    let launch_item = if display_name.starts_with("path:") {
        // This is a PATH executable with conflict resolution
        let base_name = &display_name[5..];
        launch_items
            .iter()
            .find(
                |item| matches!(&item.item, LaunchItem::PathExecutable(name) if name == base_name),
            )
            .map(|item| &item.item)
    } else {
        // This is a desktop app or PATH executable without conflict
        launch_items
            .iter()
            .find(|item| item.item.display_name() == display_name)
            .map(|item| &item.item)
    };

    match launch_item {
        Some(LaunchItem::DesktopApp(app)) => {
            execute_desktop_app(app)?;
            println!("Launched: {}", app.name);
        }
        Some(LaunchItem::PathExecutable(name)) => {
            execute_path_executable(name)?;
            println!("Launched: {}", name);
        }
        None => {
            return Err(anyhow::anyhow!("Launch item not found: {}", display_name));
        }
    }

    Ok(())
}

/// Execute a desktop application
fn execute_desktop_app(app: &crate::launch::types::DesktopApp) -> Result<()> {
    let exec_cmd = app.expand_exec(&[]);

    // Parse the exec command into command and arguments
    let parts: Vec<&str> = exec_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err(anyhow::anyhow!("Empty exec command in desktop file"));
    }

    let cmd_name = parts[0];
    let args: Vec<&str> = parts[1..].to_vec();

    let mut cmd = if app.terminal {
        // If it's a terminal app, run it in a terminal
        let terminal_cmd = std::env::var("TERMINAL").unwrap_or_else(|_| "kitty".to_string());
        let mut full_cmd = Command::new(terminal_cmd);
        full_cmd.arg("--").arg(cmd_name);
        for arg in args {
            full_cmd.arg(arg);
        }
        full_cmd
    } else {
        // Regular GUI application
        let mut cmd = Command::new(cmd_name);
        for arg in args {
            cmd.arg(arg);
        }
        cmd
    };

    // Redirect stdout/stderr to /dev/null for clean background execution
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    // Spawn process in background with detachment
    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "Failed to launch desktop app {}: {}",
            app.name,
            e
        )),
    }
}

/// Execute a PATH executable
fn execute_path_executable(executable_name: &str) -> Result<()> {
    let mut cmd = Command::new(executable_name);

    // Redirect stdout/stderr to /dev/null for clean background execution
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    // Spawn process in background with detachment
    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "Failed to launch executable {}: {}",
            executable_name,
            e
        )),
    }
}
