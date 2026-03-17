use super::{ScratchpadProvider, ScratchpadWindowInfo, create_terminal_process};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::env;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub struct InstantWM;

/// Check if we're running on instantWM (via env var)
fn is_on_instantwm() -> bool {
    env::var("INSTANTWM").is_ok()
}

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
        let name = config.name.clone();

        // Get list of windows before spawning
        let windows_before = get_window_ids()?;

        // Spawn the terminal
        create_terminal_process(config)?;

        // Wait for a new window to appear and get focus
        let min_delay = Duration::from_millis(50);
        let max_delay = Duration::from_millis(500);
        let total_timeout = Duration::from_secs(5);
        let start = std::time::Instant::now();
        let mut delay = min_delay;
        let mut new_window_seen = false;

        while start.elapsed() < total_timeout {
            let windows_after = get_window_ids()?;

            // Check if there's a new window
            let new_windows: Vec<_> = windows_after
                .iter()
                .filter(|id| !windows_before.contains(id))
                .collect();

            if !new_windows.is_empty() {
                new_window_seen = true;
                // Found a new window - give it time to get focus
                thread::sleep(Duration::from_millis(200));

                // Now try to create the scratchpad from the focused window
                if instantwmctl(&["scratchpad", "create", &name]).is_ok() {
                    thread::sleep(Duration::from_millis(100));
                    if is_scratchpad_registered(&name)? {
                        return Ok(());
                    }
                }
            }

            thread::sleep(delay);
            delay = (delay * 2).min(max_delay);
        }

        // Check if it was registered
        if is_scratchpad_registered(&name)? {
            return Ok(());
        }

        if new_window_seen {
            Err(anyhow::anyhow!(
                "Window appeared but scratchpad registration failed"
            ))
        } else {
            Err(anyhow::anyhow!("Terminal window did not appear"))
        }
    }
}

/// Get list of window IDs from instantwmctl
fn get_window_ids() -> Result<Vec<String>> {
    let output = Command::new("instantwmctl")
        .args(["window", "list"])
        .output()
        .context("Failed to execute instantwmctl window list")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut ids = Vec::new();

    // Parse JSON output: {"windows": [{"id": 123, ...}, ...]}
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
        if let Some(windows) = json.get("windows").and_then(|w| w.as_array()) {
            for window in windows {
                if let Some(id) = window.get("id").and_then(|i| i.as_u64()) {
                    ids.push(id.to_string());
                }
            }
        }
    }

    Ok(ids)
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

/// Check if a window with the given class exists using instantwmctl
fn is_window_in_tree(window_class: &str) -> Result<bool> {
    // On instantWM, always use instantwmctl (works on both X11 and Wayland)
    if is_on_instantwm() {
        let output = Command::new("instantwmctl")
            .args(["window", "list"])
            .output()
            .context("Failed to execute instantwmctl window list")?;

        if !output.status.success() {
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Match by window class in title (terminal windows typically have the class in title)
        return Ok(stdout.to_lowercase().contains(&window_class.to_lowercase()));
    }

    // Fallback: shouldn't reach here since this is InstantWM provider
    // But keep xwininfo as last resort
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

fn is_scratchpad_registered(name: &str) -> Result<bool> {
    let output = instantwmctl_output(&["scratchpad", "list"])?;
    // instantWM outputs format like: "* scratchpadname (visible)" or "  scratchpadname (hidden)"
    // We need to extract the name and check if it matches
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Remove the leading marker (* or space) and status
        if let Some(name_part) = line.strip_prefix("* ").or_else(|| line.strip_prefix("  ")) {
            // Now we have "name (visible)" or "name (hidden)"
            if let Some((scratchpad_name, _)) = name_part.split_once('(') {
                let scratchpad_name = scratchpad_name.trim();
                if scratchpad_name == name {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn parse_scratchpad_list(output: &str) -> Result<Vec<ScratchpadWindowInfo>> {
    let mut windows = Vec::new();

    // instantWM outputs format like:
    // * scratchpadname (visible)
    //   music (hidden)
    // or "no scratchpads" if none exist

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line == "no scratchpads" {
            continue;
        }

        // Remove leading marker (* or space)
        let (name_with_status, visible) = if let Some(stripped) = line.strip_prefix("* ").or_else(|| line.strip_prefix("  ")) {
            // Now we have "name (visible)" or "name (hidden)"
            if let Some((name, status)) = stripped.split_once('(') {
                let name = name.trim();
                let visible = status.trim().trim_end_matches(')') == "visible";
                (name, visible)
            } else {
                (stripped.trim(), false)
            }
        } else {
            continue;
        };

        windows.push(ScratchpadWindowInfo {
            name: name_with_status.to_string(),
            window_class: format!("scratchpad_{}", name_with_status),
            title: name_with_status.to_string(),
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
