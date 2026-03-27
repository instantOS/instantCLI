use super::{ScratchpadProvider, ScratchpadWindowInfo, create_terminal_process};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub struct InstantWM;

#[derive(Debug, Deserialize)]
struct InstantWmScratchpadInfo {
    name: String,
    visible: bool,
}

#[derive(Debug, Deserialize)]
struct InstantWmWindowInfo {
    id: u64,
}

impl ScratchpadProvider for InstantWM {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        if is_scratchpad_registered(&config.name)? {
            instantwmctl(&["scratchpad", "show", &config.name])?;
        } else {
            self.create_and_wait(config, ScratchpadStatus::Shown)?;
        }
        Ok(())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        instantwmctl(&["scratchpad", "hide", &config.name])
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        if is_scratchpad_registered(&config.name)? {
            instantwmctl(&["scratchpad", "toggle", &config.name])?;
        } else {
            self.create_and_wait(config, ScratchpadStatus::Shown)?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        let scratchpads = get_scratchpad_list(None)?;
        Ok(scratchpads
            .into_iter()
            .map(|scratchpad| ScratchpadWindowInfo {
                window_class: format!("scratchpad_{}", scratchpad.name),
                title: scratchpad.name.clone(),
                name: scratchpad.name,
                visible: scratchpad.visible,
            })
            .collect())
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        is_scratchpad_registered(&config.name)
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        Ok(get_scratchpad_info(&config.name)?.is_some_and(|scratchpad| scratchpad.visible))
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
    fn create_and_wait(
        &self,
        config: &ScratchpadConfig,
        initial_status: ScratchpadStatus,
    ) -> Result<()> {
        let windows_before = get_window_ids()?;
        create_terminal_process(config)?;

        let min_delay = Duration::from_millis(20);
        let max_delay = Duration::from_millis(200);
        let total_timeout = Duration::from_secs(5);
        let start = std::time::Instant::now();
        let mut delay = min_delay;
        let mut new_window_seen = false;

        while start.elapsed() < total_timeout {
            let windows_after = get_window_ids()?;
            if let Some(window_id) = find_new_window(&windows_before, &windows_after) {
                new_window_seen = true;
                create_scratchpad(window_id, &config.name, initial_status)?;
                thread::sleep(Duration::from_millis(40));

                if let Some(scratchpad) = get_scratchpad_info(&config.name)? {
                    if !matches!(initial_status, ScratchpadStatus::Shown) || scratchpad.visible {
                        return Ok(());
                    }
                    instantwmctl(&["scratchpad", "show", &config.name])?;
                    thread::sleep(Duration::from_millis(30));
                    if get_scratchpad_info(&config.name)?.is_some_and(|sp| sp.visible) {
                        return Ok(());
                    }
                }
            }

            thread::sleep(delay);
            delay = (delay * 2).min(max_delay);
        }

        if is_scratchpad_registered(&config.name)? {
            Ok(())
        } else if new_window_seen {
            Err(anyhow::anyhow!(
                "terminal appeared but instantWM scratchpad registration failed"
            ))
        } else {
            Err(anyhow::anyhow!("terminal window did not appear"))
        }
    }
}

#[derive(Clone, Copy)]
enum ScratchpadStatus {
    Hidden,
    Shown,
}

impl ScratchpadStatus {
    fn as_cli_arg(self) -> &'static str {
        match self {
            ScratchpadStatus::Hidden => "hidden",
            ScratchpadStatus::Shown => "shown",
        }
    }
}

fn find_new_window(before: &[u64], after: &[u64]) -> Option<u64> {
    let before_set: HashSet<u64> = before.iter().copied().collect();
    after
        .iter()
        .rev()
        .copied()
        .find(|window_id| !before_set.contains(window_id))
}

fn create_scratchpad(window_id: u64, name: &str, status: ScratchpadStatus) -> Result<()> {
    let window_id = window_id.to_string();
    instantwmctl(&[
        "scratchpad",
        "create",
        name,
        "--window-id",
        &window_id,
        "--status",
        status.as_cli_arg(),
    ])
}

fn instantwmctl(args: &[&str]) -> Result<()> {
    let output = Command::new("instantwmctl")
        .args(args)
        .output()
        .context("Failed to execute instantwmctl")?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "instantwmctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

fn instantwmctl_json_output<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Result<T> {
    let output = Command::new("instantwmctl")
        .arg("--json")
        .args(args)
        .output()
        .context("Failed to execute instantwmctl")?;

    if !output.status.success() {
        anyhow::bail!(
            "instantwmctl --json {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "Failed to parse instantwmctl --json {} output",
            args.join(" ")
        )
    })
}

fn get_window_ids() -> Result<Vec<u64>> {
    let windows: Vec<InstantWmWindowInfo> = instantwmctl_json_output(&["window", "list"])?;
    Ok(windows.into_iter().map(|window| window.id).collect())
}

fn get_scratchpad_list(name: Option<&str>) -> Result<Vec<InstantWmScratchpadInfo>> {
    let mut args = vec!["scratchpad", "status"];
    if let Some(name) = name {
        args.push(name);
    }
    instantwmctl_json_output(&args)
}

fn get_scratchpad_info(name: &str) -> Result<Option<InstantWmScratchpadInfo>> {
    Ok(get_scratchpad_list(Some(name))?.into_iter().next())
}

fn is_scratchpad_registered(name: &str) -> Result<bool> {
    Ok(get_scratchpad_info(name)?.is_some())
}

pub fn reload_config() -> Result<()> {
    instantwmctl(&["reload"])
}

pub fn set_mode(mode_name: &str) -> Result<()> {
    instantwmctl(&["mode", "set", mode_name])
}

pub fn list_modes() -> Result<String> {
    let output = Command::new("instantwmctl")
        .args(["mode", "list"])
        .output()
        .context("Failed to execute instantwmctl")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        anyhow::bail!(
            "instantwmctl mode list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_new_window_from_delta() {
        let before = vec![1, 2, 3];
        let after = vec![1, 2, 3, 7];
        assert_eq!(find_new_window(&before, &after), Some(7));
    }

    #[test]
    fn prefers_latest_new_window_when_multiple_appear() {
        let before = vec![1, 2];
        let after = vec![1, 2, 5, 8];
        assert_eq!(find_new_window(&before, &after), Some(8));
    }

    #[test]
    fn returns_none_when_no_new_window_exists() {
        let before = vec![1, 2, 3];
        let after = vec![1, 2, 3];
        assert_eq!(find_new_window(&before, &after), None);
    }

    #[test]
    fn scratchpad_status_cli_arg_matches_instantwmctl_values() {
        assert_eq!(ScratchpadStatus::Hidden.as_cli_arg(), "hidden");
        assert_eq!(ScratchpadStatus::Shown.as_cli_arg(), "shown");
    }
}
