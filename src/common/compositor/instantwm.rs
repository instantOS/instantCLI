use super::{ScratchpadProvider, ScratchpadWindowInfo, create_terminal_process};
use crate::scratchpad::config::ScratchpadConfig;
use crate::common::display_server::DisplayServer;
use anyhow::{Context, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub struct InstantWM;

impl ScratchpadProvider for InstantWM {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        if is_scratchpad_registered(&config.name)? {
            instantwmctl(&["scratchpad", "show", &config.name])?;
            return Ok(());
        }

        self.create_and_wait(config)?;
        Ok(())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        instantwmctl(&["scratchpad", "hide", &config.name])
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        if is_scratchpad_registered(&config.name)? {
            instantwmctl(&["scratchpad", "toggle", &config.name])?;
        } else {
            self.create_and_wait(config)?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        let output = instantwmctl_output(&["scratchpad", "list"])?;
        parse_scratchpad_list(&output)
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        is_scratchpad_registered(&config.name)
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        let output = instantwmctl_output(&["scratchpad", "status", &config.name])?;
        Ok(output.trim() == "visible")
    }

    fn show_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        instantwmctl(&["scratchpad", "show", &config.name])
    }

    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        instantwmctl(&["scratchpad", "hide", &config.name])
    }

    fn supports_scratchpad(&self) -> bool {
        true
    }
}

impl InstantWM {
    fn create_and_wait(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        let name = config.name.clone();
        create_terminal_process(config)?;

        let min_delay = Duration::from_millis(20);
        let max_delay = Duration::from_millis(300);
        let total_timeout = Duration::from_secs(5);
        let start = std::time::Instant::now();
        let mut delay = min_delay;
        let mut window_seen = false;

        while start.elapsed() < total_timeout {
            if is_window_in_tree(&window_class)? {
                window_seen = true;
                if is_scratchpad_registered(&name)? {
                    thread::sleep(Duration::from_millis(30));
                    return Ok(());
                }
                try_register_scratchpad(&window_class, &name)?;
                thread::sleep(Duration::from_millis(50));
                if is_scratchpad_registered(&name)? {
                    return Ok(());
                }
            }
            thread::sleep(delay);
            delay = (delay * 2).min(max_delay);
        }

        if window_seen {
            Err(anyhow::anyhow!(
                "Window appeared but scratchpad registration failed"
            ))
        } else {
            Err(anyhow::anyhow!("Terminal window did not appear"))
        }
    }
}

fn instantwmctl(args: &[&str]) -> Result<()> {
    let status = Command::new("instantwmctl")
        .args(args)
        .status()
        .context("Failed to execute instantwmctl")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "instantwmctl {} failed with exit code {}",
            args.join(" "),
            status.code().unwrap_or(-1)
        )
    }
}

fn instantwmctl_output(args: &[&str]) -> Result<String> {
    let output = Command::new("instantwmctl")
        .args(args)
        .output()
        .context("Failed to execute instantwmctl")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        anyhow::bail!(
            "instantwmctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

pub fn reload_config() -> Result<()> {
    instantwmctl(&["reload"])
}

pub fn set_mode(mode_name: &str) -> Result<()> {
    instantwmctl(&["mode", "set", mode_name])
}

pub fn list_modes() -> Result<String> {
    instantwmctl_output(&["mode", "list"])
}

pub fn get_current_mode() -> Result<String> {
    let output = list_modes()?;
    for line in output.lines() {
        if line.starts_with("* ") || line.contains("(current)") {
            return Ok(line
                .trim_start_matches("* ")
                .trim_end_matches(" (current)")
                .to_string());
        }
    }
    Ok("default".to_string())
}

fn try_register_scratchpad(window_class: &str, name: &str) -> Result<()> {
    if let Some(window_id) = find_window_by_class(window_class)? {
        focus_window_by_id(&window_id)?;
        thread::sleep(Duration::from_millis(50));
    }
    instantwmctl(&["scratchpad", "create", name])?;
    thread::sleep(Duration::from_millis(50));
    instantwmctl(&["scratchpad", "show", name])?;
    Ok(())
}

fn is_window_in_tree(window_class: &str) -> Result<bool> {
    // Check if we're on Wayland - xwininfo won't work
    if DisplayServer::detect() == DisplayServer::Wayland {
        // On Wayland, use instantwmctl window list instead
        return is_window_in_tree_wayland(window_class);
    }

    // On X11, use xwininfo
    let output = Command::new("xwininfo")
        .args(["-tree", "-root"])
        .output()
        .context("Failed to execute xwininfo")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.contains(window_class))
    } else {
        Ok(false)
    }
}

/// Check if a window exists using instantwmctl (works on both X11 and Wayland)
fn is_window_in_tree_wayland(window_class: &str) -> Result<bool> {
    let output = Command::new("instantwmctl")
        .args(["window", "list"])
        .output()
        .context("Failed to execute instantwmctl window list")?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Match by window title (the terminal window title should contain the class)
    Ok(stdout.to_lowercase().contains(&window_class.to_lowercase()))
}

fn find_window_by_class(window_class: &str) -> Result<Option<String>> {
    // Check if we're on Wayland - xwininfo won't work
    if DisplayServer::detect() == DisplayServer::Wayland {
        return find_window_by_class_wayland(window_class);
    }

    // On X11, use xwininfo
    let output = Command::new("xwininfo")
        .args(["-tree", "-root"])
        .output()
        .context("Failed to execute xwininfo")?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains(window_class)
            && let Some(hex_id) = line.split_whitespace().next()
        {
            return Ok(Some(hex_id.to_string()));
        }
    }

    Ok(None)
}

/// Find window by class using instantwmctl (Wayland)
fn find_window_by_class_wayland(window_class: &str) -> Result<Option<String>> {
    let output = Command::new("instantwmctl")
        .args(["window", "list"])
        .output()
        .context("Failed to execute instantwmctl window list")?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse JSON output to find window by title
    // The JSON format is: {"windows": [{"id": ..., "title": "...", ...}]}
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
        if let Some(windows) = json.get("windows").and_then(|w| w.as_array()) {
            for window in windows {
                if let Some(title) = window.get("title").and_then(|t| t.as_str()) {
                    if title.to_lowercase().contains(&window_class.to_lowercase()) {
                        if let Some(id) = window.get("id").and_then(|i| i.as_u64()) {
                            return Ok(Some(id.to_string()));
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

fn focus_window_by_id(window_id: &str) -> Result<()> {
    // Check if we're on Wayland - wmctrl won't work
    if DisplayServer::detect() == DisplayServer::Wayland {
        // On Wayland, we don't need to focus explicitly - instantwmctl scratchpad create
        // will use the currently focused window
        return Ok(());
    }

    // On X11, use wmctrl
    let status = Command::new("wmctrl")
        .args(["-i", "-a", window_id])
        .status()
        .context("Failed to execute wmctrl")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("wmctrl failed to focus window")
    }
}

fn is_scratchpad_registered(name: &str) -> Result<bool> {
    let output = instantwmctl_output(&["scratchpad", "list"])?;
    for line in output.lines() {
        if line.starts_with(&format!("{}:", name)) || line == name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_scratchpad_list(output: &str) -> Result<Vec<ScratchpadWindowInfo>> {
    let mut windows = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (name, visible) = if let Some((name, state)) = line.split_once(':') {
            (name, state == "visible")
        } else {
            (line, false)
        };

        windows.push(ScratchpadWindowInfo {
            name: name.to_string(),
            window_class: format!("scratchpad_{}", name),
            title: name.to_string(),
            visible,
        });
    }

    Ok(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_scratchpad_list() {
        let output = "default:visible\ntest:hidden";
        let windows = parse_scratchpad_list(output).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].name, "default");
        assert!(windows[0].visible);
        assert_eq!(windows[1].name, "test");
        assert!(!windows[1].visible);
    }

    #[test]
    fn test_parse_scratchpad_list_empty() {
        let output = "";
        let windows = parse_scratchpad_list(output).unwrap();
        assert!(windows.is_empty());
    }

    #[test]
    fn test_parse_scratchpad_list_single() {
        let output = "mymenu:visible";
        let windows = parse_scratchpad_list(output).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "mymenu");
        assert!(windows[0].visible);
    }
}
