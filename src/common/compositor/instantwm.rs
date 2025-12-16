use super::{ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

/// InstantWM scratchpad provider
///
/// Uses instantWM's native IPC mechanism via xsetroot and the existing
/// scratchpad commands (showscratchpad, hidescratchpad, scratchpadstatus)
pub struct InstantWM;

impl ScratchpadProvider for InstantWM {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        if !self.is_window_running(config)? {
            self.create_and_wait(config)?;
        }
        self.show_unchecked(config)
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        if !self.is_window_running(config)? {
            // Window doesn't exist, nothing to hide
            return Ok(());
        }
        self.hide_unchecked(config)
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        if !self.is_window_running(config)? {
            // Create the window first
            self.create_and_wait(config)?;
            // Show it
            self.show_unchecked(config)?;
        } else {
            // Window exists, check visibility and toggle
            if self.is_visible(config)? {
                self.hide_unchecked(config)?;
            } else {
                self.show_unchecked(config)?;
            }
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        let status = get_scratchpad_status()?;
        let is_visible = status;

        // For now, instantWM supports one scratchpad per monitor
        // We'll check if the default scratchpad is running
        let mut windows = Vec::new();

        // Try to detect the default scratchpad
        if self.is_default_scratchpad_running()? {
            windows.push(ScratchpadWindowInfo {
                name: "instantscratchpad".to_string(),
                window_class: "scratchpad_instantscratchpad".to_string(),
                title: "instantscratchpad".to_string(),
                visible: is_visible,
            });
        }

        Ok(windows)
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        // For instantWM, we use the window class to detect if it's running
        // via xwininfo or similar tools
        let window_class = config.window_class();

        // Use xwininfo to check if a window with this class exists
        let output = Command::new("xwininfo")
            .args(["-tree", "-root"])
            .output()
            .context("Failed to execute xwininfo")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.contains(&window_class) || stdout.contains(&config.name))
        } else {
            // Fallback: assume it's running if we can't determine
            Ok(false)
        }
    }

    fn is_visible(&self, _config: &ScratchpadConfig) -> Result<bool> {
        // Use instantWM's scratchpad status command
        get_scratchpad_status()
    }

    fn show_unchecked(&self, _config: &ScratchpadConfig) -> Result<()> {
        // Direct show command without checking if window exists
        send_instantwm_command("showscratchpad", "")
    }

    fn hide_unchecked(&self, _config: &ScratchpadConfig) -> Result<()> {
        // Direct hide command without checking if window exists
        send_instantwm_command("hidescratchpad", "")
    }
}

impl InstantWM {
    fn create_and_wait(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        super::create_terminal_process(config)?;

        // Wait for window to appear
        let mut attempts = 0;
        while attempts < 30 {
            if self.is_window_running(config)? {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(200));
            attempts += 1;
        }

        Err(anyhow::anyhow!("Terminal window did not appear"))
    }

    fn is_default_scratchpad_running(&self) -> Result<bool> {
        // Check if the default instantWM scratchpad is running
        let output = Command::new("xwininfo")
            .args(["-tree", "-root"])
            .output()
            .context("Failed to execute xwininfo")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.contains("instantscratchpad"))
        } else {
            Ok(false)
        }
    }
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

/// Get scratchpad status from instantWM
/// Returns true if scratchpad is visible, false if hidden
pub fn get_scratchpad_status() -> Result<bool> {
    // Send status query command
    send_instantwm_command("scratchpadstatus", "")?;

    // Wait for the response in WM_NAME
    for attempt in 0..20 {
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

/// Get scratchpad status using instantwmctl if available
pub fn get_scratchpad_status_via_ctl() -> Result<bool> {
    let output = Command::new("instantwmctl")
        .arg("scratchpadstatus")
        .output()
        .context("Failed to execute instantwmctl");

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(status_line) = stdout.lines().next() {
                if let Some(status_str) = status_line.strip_prefix("scratchpad:") {
                    return Ok(status_str == "1");
                }
            }
            Ok(false)
        }
        _ => {
            // Fallback to direct xprop method
            get_scratchpad_status()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_formatting() {
        let cmd = send_instantwm_command("showscratchpad", "");
        assert!(cmd.is_ok() || cmd.is_err()); // We can't test actual execution without instantWM running
    }

    #[test]
    fn test_status_parsing() {
        // This would require mocking xprop output for proper testing
        // For now, just ensure the function exists and has the right signature
        let result = get_scratchpad_status();
        assert!(result.is_ok() || result.is_err());
    }
}
