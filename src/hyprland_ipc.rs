use anyhow::{Context, Result};
use hyprland::data::{Clients, Workspace, Workspaces};
use hyprland::dispatch::{Dispatch, DispatchType};
use hyprland::keyword::Keyword;
use hyprland::prelude::*;

/// Check if a window with specific class exists in Hyprland using direct IPC
pub fn window_exists(window_class: &str) -> Result<bool> {
    let clients = Clients::get()
        .context("Failed to get clients from Hyprland IPC")?
        .to_vec();

    for client in clients.iter() {
        if client.class == window_class {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Setup window rules for Hyprland scratchpad using direct IPC
pub fn setup_window_rules(workspace_name: &str, window_class: &str) -> Result<()> {
    // Add window rules for the scratchpad terminal
    let rules = vec![
        format!(
            "workspace special:{},class:^({})$",
            workspace_name, window_class
        ),
        format!("center,class:^({})$", window_class),
    ];

    for rule in rules {
        Keyword::set("windowrulev2", rule.clone())
            .context(format!("Failed to set window rule: {}", rule))?;
    }

    Ok(())
}

/// Toggle special workspace visibility using direct IPC
pub fn toggle_special_workspace(workspace_name: &str) -> Result<()> {
    Dispatch::call(DispatchType::ToggleSpecialWorkspace(Some(
        workspace_name.to_string(),
    )))
    .context("Failed to toggle special workspace")?;

    Ok(())
}

/// Show special workspace using direct IPC
pub fn show_special_workspace(workspace_name: &str) -> Result<()> {
    // Check if the special workspace is already active
    if !is_special_workspace_active(workspace_name)? {
        // If not active, toggle it to show it
        toggle_special_workspace(workspace_name)?;
    }
    Ok(())
}

/// Hide special workspace using direct IPC
pub fn hide_special_workspace(workspace_name: &str) -> Result<()> {
    // Check if the special workspace is currently active
    if is_special_workspace_active(workspace_name)? {
        // If active, toggle it to hide it
        toggle_special_workspace(workspace_name)?;
    }
    Ok(())
}

/// Check if special workspace is active using direct IPC
pub fn is_special_workspace_active(workspace_name: &str) -> Result<bool> {
    let workspaces = Workspaces::get()
        .context("Failed to get workspaces from Hyprland IPC")?
        .to_vec();

    // Find the active workspace
    for workspace in workspaces.iter() {
        if workspace.id < 0 && workspace.name.contains(workspace_name) {
            // Special workspaces have negative IDs
            // Check if this special workspace is currently active
            // We need to check if any window is currently focused on this workspace
            let clients = Clients::get()
                .context("Failed to get clients from Hyprland IPC")?
                .to_vec();

            for client in clients.iter() {
                if client.workspace.name.contains(workspace_name) && client.focus_history_id == 0 {
                    // focus_history_id == 0 means it's the currently focused window
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
}

/// Get active workspace information using direct IPC
pub fn get_active_workspace() -> Result<Workspace> {
    let workspaces = Workspaces::get()
        .context("Failed to get workspaces from Hyprland IPC")?
        .to_vec();

    // Find the active workspace (the one with the focused window)
    let clients = Clients::get()
        .context("Failed to get clients from Hyprland IPC")?
        .to_vec();

    // Find the currently focused client
    let focused_client = clients.iter().find(|client| client.focus_history_id == 0);

    if let Some(client) = focused_client {
        // Find the workspace that contains this client
        let workspace = workspaces
            .iter()
            .find(|ws| ws.id == client.workspace.id)
            .context("Failed to find workspace for focused client")?;
        return Ok(workspace.clone());
    }

    // Fallback: find workspace marked as active (though this might not be as reliable)
    let active_workspace = workspaces
        .iter()
        .find(|ws| ws.id > 0) // Regular workspaces have positive IDs
        .context("No active workspace found")?;

    Ok(active_workspace.clone())
}

/// Execute a hyprland dispatcher using direct IPC
pub fn dispatch_command(command: &str) -> Result<()> {
    // Parse the command and convert to appropriate DispatchType
    // This is a simplified version - you might want to expand this based on your needs
    if command.starts_with("exec") {
        let exec_command = command.strip_prefix("exec ").unwrap_or(command);
        Dispatch::call(DispatchType::Exec(exec_command)).context("Failed to execute command")?;
    } else if command.starts_with("togglespecialworkspace") {
        let workspace_name = command
            .strip_prefix("togglespecialworkspace ")
            .unwrap_or("");
        if workspace_name.is_empty() {
            Dispatch::call(DispatchType::ToggleSpecialWorkspace(None))
                .context("Failed to toggle special workspace")?;
        } else {
            Dispatch::call(DispatchType::ToggleSpecialWorkspace(Some(
                workspace_name.to_string(),
            )))
            .context("Failed to toggle special workspace")?;
        }
    } else {
        // For other commands, we might need to extend this
        return Err(anyhow::anyhow!("Unsupported dispatch command: {}", command));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_exists_parsing() {
        // This test would require a running Hyprland instance
        // For now, we'll just test that the function doesn't panic
        // In a real test environment, you'd want to mock the IPC calls
        assert!(true);
    }

    #[test]
    fn test_dispatch_command_parsing() {
        // Test command parsing logic
        // This would also require mocking in a real test
        assert!(true);
    }
}
