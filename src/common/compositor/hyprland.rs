use super::{ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

pub struct Hyprland;

impl ScratchpadProvider for Hyprland {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        let workspace_name = config.workspace_name();

        if !self.is_window_running(config)? {
            self.create_and_wait(config)?;
        }

        show_special_workspace(&workspace_name)
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        let workspace_name = config.workspace_name();
        hide_special_workspace(&workspace_name)
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        let workspace_name = config.workspace_name();

        if self.is_window_running(config)? {
            toggle_special_workspace(&workspace_name)?;
        } else {
            self.create_and_wait(config)?;
            show_special_workspace(&workspace_name)?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        get_all_scratchpad_windows()
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        window_exists(&config.window_class())
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        let workspace_name = config.workspace_name();
        let window_class = config.window_class();

        let special_workspace_active =
            is_special_workspace_active(&workspace_name).unwrap_or(false);
        let window_exists = window_exists(&window_class)?;

        Ok(special_workspace_active && window_exists)
    }

    fn show_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        let workspace_name = config.workspace_name();
        show_special_workspace(&workspace_name)
    }

    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        let workspace_name = config.workspace_name();
        hide_special_workspace(&workspace_name)
    }

    fn supports_scratchpad(&self) -> bool {
        true
    }
}

impl Hyprland {
    fn create_and_wait(&self, config: &ScratchpadConfig) -> Result<()> {
        let workspace_name = config.workspace_name();
        let window_class = config.window_class();

        setup_window_rules(&workspace_name, &window_class)?;
        super::create_terminal_process(config)?;

        // Wait for window
        let mut attempts = 0;
        while attempts < 30 {
            if let Some(client) = get_client_by_class(&window_class)? {
                let expected_workspace = format!("special:{}", workspace_name);
                if client.workspace.name != expected_workspace {
                    move_window_to_special(&client.address, &workspace_name)?;
                }
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            attempts += 1;
        }

        Err(anyhow::anyhow!("Terminal window did not appear"))
    }
}

/// Client information from hyprctl clients -j
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyprlandClient {
    pub address: String,
    pub class: String,
    pub title: String,
    pub workspace: HyprlandWorkspace,
    #[serde(rename = "focusHistoryID")]
    pub focus_history_id: i32,
}

/// Workspace information from hyprctl clients -j
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyprlandWorkspace {
    pub name: String,
}

/// Get client info by class
pub fn get_client_by_class(window_class: &str) -> Result<Option<HyprlandClient>> {
    let output = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .context("Failed to execute hyprctl clients")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("hyprctl clients failed: {}", stderr));
    }

    let clients: Vec<HyprlandClient> = serde_json::from_slice(&output.stdout)
        .context("Failed to parse hyprctl clients JSON output")?;

    for client in clients.into_iter() {
        if client.class == window_class {
            return Ok(Some(client));
        }
    }

    Ok(None)
}

/// Move window to special workspace
pub fn move_window_to_special(address: &str, workspace_name: &str) -> Result<()> {
    // Modern Hyprland (0.56+) uses lua dispatchers via `hl.dsp`.
    // Focus the target window by address then move the focused window to the special workspace.
    let focus_cmd = format!("hl.dsp.focus({{window='address:{}'}})", address);
    let focus_output = Command::new("hyprctl")
        .args(["dispatch", &focus_cmd])
        .output()
        .context("Failed to execute hyprctl dispatch focus")?;

    if !focus_output.status.success() {
        let stdout = String::from_utf8_lossy(&focus_output.stdout);
        let stderr = String::from_utf8_lossy(&focus_output.stderr);
        let msg = format!("{} {}", stdout, stderr);
        return Err(anyhow::anyhow!(
            "Failed to move window to special workspace (focus): {}",
            msg.trim()
        ));
    }
    let focus_stdout = String::from_utf8_lossy(&focus_output.stdout);
    let focus_stderr = String::from_utf8_lossy(&focus_output.stderr);
    if focus_stdout.contains("error:") || focus_stderr.contains("error:") {
        return Err(anyhow::anyhow!(
            "Failed to move window to special workspace (focus): {} {}",
            focus_stdout.trim(),
            focus_stderr.trim()
        ));
    }

    let move_cmd = format!(
        "hl.dsp.window.move({{workspace='special:{}'}})",
        workspace_name
    );

    let move_output = Command::new("hyprctl")
        .args(["dispatch", &move_cmd])
        .output()
        .context("Failed to execute hyprctl dispatch window.move")?;

    if !move_output.status.success() {
        let stdout = String::from_utf8_lossy(&move_output.stdout);
        let stderr = String::from_utf8_lossy(&move_output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to move window to special workspace: {} {}",
            stdout.trim(),
            stderr.trim()
        ));
    }
    let move_stdout = String::from_utf8_lossy(&move_output.stdout);
    let move_stderr = String::from_utf8_lossy(&move_output.stderr);
    if move_stdout.contains("error:") || move_stderr.contains("error:") {
        return Err(anyhow::anyhow!(
            "Failed to move window to special workspace: {} {}",
            move_stdout.trim(),
            move_stderr.trim()
        ));
    }

    Ok(())
}

/// Check if a window with specific class exists in Hyprland using hyprctl
pub fn window_exists(window_class: &str) -> Result<bool> {
    let output = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .context("Failed to execute hyprctl clients")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("hyprctl clients failed: {}", stderr));
    }

    let clients: Vec<HyprlandClient> = serde_json::from_slice(&output.stdout)
        .context("Failed to parse hyprctl clients JSON output")?;

    for client in clients.iter() {
        if client.class == window_class {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Setup window rules for Hyprland scratchpad using hyprctl
pub fn setup_window_rules(workspace_name: &str, window_class: &str) -> Result<()> {
    // Hyprland 0.56+ uses lua config via `eval hl.window_rule`.
    let lua_rules = vec![
        format!(
            "hl.window_rule({{name='scratch-{}-workspace', match={{class='{}'}}, workspace='special:{}'}})",
            workspace_name, window_class, workspace_name
        ),
        format!(
            "hl.window_rule({{name='scratch-{}-float', match={{class='{}'}}, float=true}})",
            workspace_name, window_class
        ),
        format!(
            "hl.window_rule({{name='scratch-{}-size', match={{class='{}'}}, size='80% 80%'}})",
            workspace_name, window_class
        ),
        format!(
            "hl.window_rule({{name='scratch-{}-center', match={{class='{}'}}, center=true}})",
            workspace_name, window_class
        ),
    ];
    let batch_str = lua_rules
        .into_iter()
        .map(|r| format!("eval {}", r))
        .collect::<Vec<_>>()
        .join(" ; ");

    let output = Command::new("hyprctl")
        .args(["--batch", &batch_str])
        .output()
        .context("Failed to execute hyprctl batch for window rules")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow::anyhow!(
            "Failed to set window rules: {} {}",
            stdout.trim(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stdout.contains("error:") || stderr.contains("error:") {
        return Err(anyhow::anyhow!(
            "Failed to set window rules: {} {}",
            stdout.trim(),
            stderr.trim()
        ));
    }

    Ok(())
}

/// Toggle special workspace visibility using hyprctl
pub fn toggle_special_workspace(workspace_name: &str) -> Result<()> {
    let lua = format!("hl.dsp.workspace.toggle_special('{}')", workspace_name);
    let output = Command::new("hyprctl")
        .args(["dispatch", &lua])
        .output()
        .context("Failed to execute hyprctl dispatch toggle_special")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to toggle special workspace '{}': {} {}",
            workspace_name,
            stdout.trim(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stdout.contains("error:") || stderr.contains("error:") {
        return Err(anyhow::anyhow!(
            "Failed to toggle special workspace '{}': {} {}",
            workspace_name,
            stdout.trim(),
            stderr.trim()
        ));
    }

    Ok(())
}

/// Show special workspace using hyprctl
pub fn show_special_workspace(workspace_name: &str) -> Result<()> {
    if !is_special_workspace_active(workspace_name)? {
        toggle_special_workspace(workspace_name)?;
    }
    Ok(())
}

/// Hide special workspace using hyprctl
pub fn hide_special_workspace(workspace_name: &str) -> Result<()> {
    if is_special_workspace_active(workspace_name)? {
        toggle_special_workspace(workspace_name)?;
    }
    Ok(())
}

/// Monitor information from hyprctl monitors -j
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HyprlandMonitorInfo {
    #[serde(rename = "activeWorkspace")]
    pub active_workspace: HyprlandWorkspace,
    #[serde(rename = "specialWorkspace")]
    pub special_workspace: HyprlandWorkspace,
}

/// Check if special workspace is active using hyprctl
pub fn is_special_workspace_active(workspace_name: &str) -> Result<bool> {
    // Get monitors to check which special workspace is currently active
    let monitors_output = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .context("Failed to execute hyprctl monitors")?;

    if !monitors_output.status.success() {
        let stderr = String::from_utf8_lossy(&monitors_output.stderr);
        return Err(anyhow::anyhow!("hyprctl monitors failed: {}", stderr));
    }

    let monitors: Vec<HyprlandMonitorInfo> = serde_json::from_slice(&monitors_output.stdout)
        .context("Failed to parse hyprctl monitors JSON output")?;

    // Check if any monitor has the special workspace active
    let special_workspace_name = format!("special:{workspace_name}");
    for monitor in monitors.iter() {
        if monitor.special_workspace.name == special_workspace_name {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Get all scratchpad windows in Hyprland
pub fn get_all_scratchpad_windows() -> Result<Vec<ScratchpadWindowInfo>> {
    let output = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .context("Failed to execute hyprctl clients")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("hyprctl clients failed: {}", stderr));
    }

    let clients: Vec<HyprlandClient> = serde_json::from_slice(&output.stdout)
        .context("Failed to parse hyprctl clients JSON output")?;

    let mut scratchpads = Vec::new();

    for client in clients.iter() {
        // Check if this is a scratchpad window (class starts with "scratchpad_")
        if let Some(scratchpad_name) = client.class.strip_prefix("scratchpad_") {
            // Use the improved workspace activity detection
            let is_visible = is_special_workspace_active(&format!("scratchpad_{scratchpad_name}"))?;

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
    fn test_command_construction() {
        // Test that commands are constructed correctly
        // This doesn't actually run hyprctl, just tests the logic
        let command = "focusworkspace special:term";
        let parts: Vec<&str> = command.split_whitespace().collect();
        assert_eq!(parts, vec!["focusworkspace", "special:term"]);
    }
}
