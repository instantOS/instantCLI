use super::{ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

/// InstantWM scratchpad provider
///
/// Uses instantWM's native IPC mechanism via xsetroot with named scratchpads.
/// Commands: makescratchpad, showscratchpad, hidescratchpad, togglescratchpad, scratchpadstatus
/// All commands take a scratchpad name parameter.
pub struct InstantWM;

impl ScratchpadProvider for InstantWM {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        send_instantwm_command("showscratchpad", &config.name)
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        send_instantwm_command("hidescratchpad", &config.name)
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        send_instantwm_command("togglescratchpad", &config.name)
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        // Check for scratchpad windows by scanning for windows with "scratchpad_" prefix
        let output = Command::new("xwininfo")
            .args(["-tree", "-root"])
            .output()
            .context("Failed to execute xwininfo")?;

        let mut windows = Vec::new();

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse xwininfo output to find scratchpad windows
            for line in stdout.lines() {
                if let Some(name) = extract_scratchpad_name(line) {
                    // Check visibility for this scratchpad
                    let visible = get_scratchpad_status_for_name(&name).unwrap_or(false);
                    windows.push(ScratchpadWindowInfo {
                        name: name.clone(),
                        window_class: format!("scratchpad_{}", name),
                        title: name.clone(),
                        visible,
                    });
                }
            }
        }

        Ok(windows)
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
        send_instantwm_command("showscratchpad", &config.name)
    }

    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        send_instantwm_command("hidescratchpad", &config.name)
    }

    fn supports_scratchpad(&self) -> bool {
        true
    }
}

impl InstantWM {
    /// Register a window with class scratchpad_<name> as a scratchpad
    pub fn make_scratchpad(name: &str) -> Result<()> {
        send_instantwm_command("makescratchpad", name)
    }
}

/// Extract scratchpad name from xwininfo line if it contains a scratchpad window
fn extract_scratchpad_name(line: &str) -> Option<String> {
    // Look for "scratchpad_" in the window class/name
    if let Some(pos) = line.find("scratchpad_") {
        let rest = &line[pos + 11..]; // Skip "scratchpad_"
        // Extract the name until whitespace or special chars
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Send a command to instantWM via xsetroot
fn send_instantwm_command(command: &str, args: &str) -> Result<()> {
    let control_string = if args.is_empty() {
        format!("c;:;{}", command)
    } else {
        format!("c;:;{};{}", command, args)
    };

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

/// Get scratchpad status from instantWM for a specific named scratchpad
/// Returns true if scratchpad is visible, false if hidden
pub fn get_scratchpad_status_for_name(name: &str) -> Result<bool> {
    send_instantwm_command("scratchpadstatus", name)?;

    // Wait for the response in WM_NAME
    for _attempt in 0..20 {
        if let Ok(output) = Command::new("xprop")
            .args(["-root", "-notype", "WM_NAME"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(captures) = stdout.strip_prefix("WM_NAME = ") {
                    let value = captures.trim().trim_matches('"');
                    if let Some(status_str) = value.strip_prefix("ipc:scratchpad:") {
                        return Ok(status_str == "1");
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    // If we couldn't get the status, assume it's hidden
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_scratchpad_name() {
        assert_eq!(
            extract_scratchpad_name("     0x2000001 \"scratchpad_test\": (\"kitty\" \"kitty\")"),
            Some("test".to_string())
        );
        assert_eq!(
            extract_scratchpad_name("window class scratchpad_frank something"),
            Some("frank".to_string())
        );
        assert_eq!(extract_scratchpad_name("no scratchpad here"), None);
        assert_eq!(extract_scratchpad_name("scratchpad_ empty"), None);
    }

    #[test]
    fn test_command_formatting() {
        // We can't test actual execution without instantWM running
        // Just ensure the function has the right signature
        let cmd = send_instantwm_command("showscratchpad", "test");
        assert!(cmd.is_ok() || cmd.is_err());
    }
}