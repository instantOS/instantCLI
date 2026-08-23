use super::{ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::OnceLock;

pub struct Hyprland;

impl ScratchpadProvider for Hyprland {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        let workspace_name = config.workspace_name();

        if !self.is_window_running(config)? {
            self.spawn_scratchpad(config)?;
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
            self.spawn_scratchpad(config)?;
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
    fn spawn_scratchpad(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        let workspace_name = config.workspace_name();

        ensure_scratchpad_rules()?;

        // Spawn terminal directly on the special workspace via Hyprland's exec dispatcher.
        // This is cleaner than spawning via nohup + polling + move: Hyprland itself
        // places the window on `special:<workspace>` with the generic floating/size/center rules.
        let term_cmd = config.terminal_command();
        // Escape single quotes for lua string literal
        let escaped = term_cmd.replace('\'', "\\'");
        let exec_lua = format!(
            "hl.dsp.exec_cmd('{}', {{workspace='special:{}'}})",
            escaped, workspace_name
        );

        let output = Command::new("hyprctl")
            .args(["dispatch", &exec_lua])
            .output()
            .context("Failed to execute hyprctl dispatch exec_cmd")?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to spawn scratchpad terminal: {} {}",
                stdout.trim(),
                stderr.trim()
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stdout.contains("error:") || stderr.contains("error:") {
            return Err(anyhow::anyhow!(
                "Failed to spawn scratchpad terminal: {} {}",
                stdout.trim(),
                stderr.trim()
            ));
        }

        // Wait for window to appear (floating rules are already applied generically)
        let mut attempts = 0;
        while attempts < 30 {
            if get_client_by_class(&window_class)?.is_some() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            attempts += 1;
        }

        Err(anyhow::anyhow!("Terminal window did not appear"))
    }
}

/// Ensure generic floating/size/center rules for all `scratchpad_*` windows.
/// This replaces the previous per-scratchpad batch of 4 window_rules. Workspace
/// assignment is now handled directly by `exec_cmd`'s `workspace` param.
fn ensure_scratchpad_rules() -> Result<()> {
    static DONE: OnceLock<()> = OnceLock::new();
    if DONE.get().is_some() {
        return Ok(());
    }

    // These rules are idempotent; re-evaluating with the same name overwrites the previous.
    // Note: size must be a table { "monitor_w * 0.8", "monitor_h * 0.8" } in lua, not the old string '80% 80%'.
    let batch = [
        "eval hl.window_rule({name='scratch-generic-float', match={class='scratchpad_.*'}, float=true})",
        "eval hl.window_rule({name='scratch-generic-size', match={class='scratchpad_.*'}, size={ 'monitor_w * 0.8', 'monitor_h * 0.8' }})",
        "eval hl.window_rule({name='scratch-generic-center', match={class='scratchpad_.*'}, center=true})",
    ]
    .join(" ; ");

    let output = Command::new("hyprctl")
        .args(["--batch", &batch])
        .output()
        .context("Failed to set generic scratchpad window rules")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to set generic scratchpad rules: {} {}",
            stdout.trim(),
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stdout.contains("error:") || stderr.contains("error:") {
        return Err(anyhow::anyhow!(
            "Failed to set generic scratchpad rules: {} {}",
            stdout.trim(),
            stderr.trim()
        ));
    }

    let _ = DONE.set(());
    Ok(())
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
        if let Some(scratchpad_name) = client.class.strip_prefix("scratchpad_") {
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
        let command = "focusworkspace special:term";
        let parts: Vec<&str> = command.split_whitespace().collect();
        assert_eq!(parts, vec!["focusworkspace", "special:term"]);
    }
}