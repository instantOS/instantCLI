use super::{ScratchpadProvider, ScratchpadWindowInfo, create_terminal_process};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub struct InstantWM;

impl ScratchpadProvider for InstantWM {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        let _ = send_instantwm_command("scratchpad-show", &config.name);

        if self.is_window_running(config)? {
            return Ok(());
        }

        self.create_and_wait(config)?;
        Ok(())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        send_instantwm_command("scratchpad-hide", &config.name)
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        if send_instantwm_command("scratchpad-toggle", &config.name).is_ok() {
            return Ok(());
        }
        self.create_and_wait(config)?;
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        send_instantwm_command("scratchpad-status", "all")?;

        let output = Command::new("xprop")
            .args(["-root", "-notype", "WM_NAME"])
            .output()
            .context("Failed to read WM_NAME")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_all_scratchpads(&stdout)
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        let window_class = config.window_class();

        let output = Command::new("xwininfo")
            .args(["-tree", "-root"])
            .output()
            .context("Failed to execute xwininfo")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.contains(&window_class))
        } else {
            Ok(false)
        }
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        get_scratchpad_status_for_name(&config.name)
    }

    fn show_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        send_instantwm_command("scratchpad-show", &config.name)
    }

    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        send_instantwm_command("scratchpad-hide", &config.name)
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

fn try_register_scratchpad(window_class: &str, name: &str) -> Result<()> {
    if let Some(window_id) = find_window_by_class(window_class)? {
        focus_window_by_id(&window_id)?;
        thread::sleep(Duration::from_millis(50));
    }
    send_instantwm_command("scratchpad-make", name)?;
    thread::sleep(Duration::from_millis(50));
    send_instantwm_command("scratchpad-show", name)?;
    Ok(())
}

fn is_window_in_tree(window_class: &str) -> Result<bool> {
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

fn find_window_by_class(window_class: &str) -> Result<Option<String>> {
    let output = Command::new("xwininfo")
        .args(["-tree", "-root"])
        .output()
        .context("Failed to execute xwininfo")?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains(window_class) {
            if let Some(hex_id) = line.split_whitespace().next() {
                return Ok(Some(hex_id.to_string()));
            }
        }
    }

    Ok(None)
}

fn focus_window_by_id(window_id: &str) -> Result<()> {
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
    send_instantwm_command("scratchpad-status", name)?;

    for _attempt in 0..3 {
        if let Ok(output) = Command::new("xprop")
            .args(["-root", "-notype", "WM_NAME"])
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(captures) = stdout.strip_prefix("WM_NAME = ") {
                let value = captures.trim().trim_matches('"');
                let expected_prefix = format!("ipc:scratchpad:{}:", name);
                if value.starts_with(&expected_prefix) {
                    return Ok(true);
                }
            }
        }
        thread::sleep(Duration::from_millis(30));
    }

    Ok(false)
}

fn send_instantwm_command(command: &str, args: &str) -> Result<()> {
    let control_string = format!("c;:;{};{}", command, args);

    let status = Command::new("xsetroot")
        .args(["-name", &control_string])
        .status()
        .context("Failed to execute xsetroot")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "xsetroot command failed with exit code {}",
            status.code().unwrap_or(-1)
        )
    }
}

pub fn get_scratchpad_status_for_name(name: &str) -> Result<bool> {
    send_instantwm_command("scratchpad-status", name)?;

    for _attempt in 0..10 {
        if let Ok(output) = Command::new("xprop")
            .args(["-root", "-notype", "WM_NAME"])
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(captures) = stdout.strip_prefix("WM_NAME = ") {
                let value = captures.trim().trim_matches('"');
                let expected_prefix = format!("ipc:scratchpad:{}:", name);
                if let Some(status_str) = value.strip_prefix(&expected_prefix) {
                    return Ok(status_str == "1");
                }
            }
        }
        thread::sleep(Duration::from_millis(30));
    }

    Ok(false)
}

fn parse_all_scratchpads(output: &str) -> Result<Vec<ScratchpadWindowInfo>> {
    let mut windows = Vec::new();

    if let Some(captures) = output.strip_prefix("WM_NAME = ") {
        let value = captures.trim().trim_matches('"');

        if value.starts_with("ipc:scratchpads:") {
            let list_part = value.strip_prefix("ipc:scratchpads:").unwrap_or("");

            if list_part == "none" {
                return Ok(windows);
            }

            for entry in list_part.split(',') {
                let parts: Vec<&str> = entry.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let name = parts[0].to_string();
                    let visible = parts[1] == "1";
                    windows.push(ScratchpadWindowInfo {
                        name: name.clone(),
                        window_class: format!("scratchpad_{}", name),
                        title: name,
                        visible,
                    });
                }
            }
        }
    }

    Ok(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_all_scratchpads() {
        let output = r#"WM_NAME = "ipc:scratchpads:default=1,test=0""#;
        let windows = parse_all_scratchpads(output).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].name, "default");
        assert!(windows[0].visible);
        assert_eq!(windows[1].name, "test");
        assert!(!windows[1].visible);
    }

    #[test]
    fn test_parse_all_scratchpads_none() {
        let output = r#"WM_NAME = "ipc:scratchpads:none""#;
        let windows = parse_all_scratchpads(output).unwrap();
        assert!(windows.is_empty());
    }

    #[test]
    fn test_parse_all_scratchpads_single() {
        let output = r#"WM_NAME = "ipc:scratchpads:mymenu=1""#;
        let windows = parse_all_scratchpads(output).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "mymenu");
        assert!(windows[0].visible);
    }
}
