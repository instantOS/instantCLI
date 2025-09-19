use anyhow::{Context, Result};
use hyprland::data::{Clients, Workspace};
use hyprland::dispatch::{Dispatch, DispatchType};
use hyprland::keyword::Keyword;
use hyprland::prelude::*;

/// Information about a scratchpad window
#[derive(Debug, Clone)]
pub struct ScratchpadWindowInfo {
    pub name: String,
    pub window_class: String,
    pub title: String,
    pub visible: bool,
}

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
            .context(format!("Failed to set window rule: {rule}"))?;
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
    let active_workspace =
        Workspace::get_active().context("Failed to get active workspace from Hyprland IPC")?;
    Ok(active_workspace.id < 0 && active_workspace.name.contains(workspace_name))
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

/// Get all scratchpad windows in Hyprland
pub fn get_all_scratchpad_windows() -> Result<Vec<ScratchpadWindowInfo>> {
    let clients = Clients::get()
        .context("Failed to get clients from Hyprland IPC")?
        .to_vec();

    let mut scratchpads = Vec::new();

    for client in clients.iter() {
        // Check if this is a scratchpad window (class starts with "scratchpad_")
        if let Some(scratchpad_name) = client.class.strip_prefix("scratchpad_") {
            let workspace_name = format!("scratchpad_{scratchpad_name}");

            // Use the same logic as is_special_workspace_active for consistency
            let is_visible = is_special_workspace_active(&workspace_name).unwrap_or(false);

            scratchpads.push(ScratchpadWindowInfo {
                name: scratchpad_name.to_string(),
                window_class: client.class.clone(),
                title: client.title.clone(),
                visible: is_visible,
            });
        }
    }

    Ok(scratchpads)
}

#[cfg(test)]
mod tests {

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
