use super::{ScratchpadProvider, ScratchpadWindowInfo, create_terminal_process};
use crate::common::instantwmctl;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::Result;
use serde::Deserialize;
use std::thread;
use std::time::Duration;

pub struct InstantWM;

#[derive(Debug, Deserialize)]
struct InstantWmScratchpadInfo {
    name: String,
    visible: bool,
}

impl ScratchpadProvider for InstantWM {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        if is_scratchpad_registered(&config.name)? {
            instantwmctl::run(["scratchpad", "show", config.name.as_str()])?;
        } else {
            self.create_and_wait(config)?;
        }
        Ok(())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        instantwmctl::run(["scratchpad", "hide", config.name.as_str()])
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        if is_scratchpad_registered(&config.name)? {
            instantwmctl::run(["scratchpad", "toggle", config.name.as_str()])?;
        } else {
            self.create_and_wait(config)?;
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
        instantwmctl::run(["scratchpad", "show", config.name.as_str()])
    }

    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        instantwmctl::run(["scratchpad", "hide", config.name.as_str()])
    }

    fn supports_scratchpad(&self) -> bool {
        true
    }
}

impl InstantWM {
    fn create_and_wait(&self, config: &ScratchpadConfig) -> Result<()> {
        create_terminal_process(config)?;

        let min_delay = Duration::from_millis(20);
        let max_delay = Duration::from_millis(200);
        let total_timeout = Duration::from_secs(5);
        let start = std::time::Instant::now();
        let mut delay = min_delay;
        let mut registration_seen = false;

        while start.elapsed() < total_timeout {
            if let Some(scratchpad) = get_scratchpad_info(&config.name)? {
                registration_seen = true;
                if scratchpad.visible {
                    return Ok(());
                }
                instantwmctl::run(["scratchpad", "show", config.name.as_str()])?;
                thread::sleep(Duration::from_millis(30));
                if get_scratchpad_info(&config.name)?.is_some_and(|scratchpad| scratchpad.visible) {
                    return Ok(());
                }
            }

            thread::sleep(delay);
            delay = (delay * 2).min(max_delay);
        }

        match get_scratchpad_info(&config.name)? {
            Some(scratchpad) if scratchpad.visible => Ok(()),
            Some(_) => Err(anyhow::anyhow!(
                "instantWM registered the scratchpad but did not make it visible"
            )),
            None if registration_seen => Err(anyhow::anyhow!(
                "the scratchpad disappeared before instantWM could make it visible"
            )),
            None => Err(anyhow::anyhow!(
                "terminal did not register its scratchpad identity with instantWM"
            )),
        }
    }
}

fn get_scratchpad_list(name: Option<&str>) -> Result<Vec<InstantWmScratchpadInfo>> {
    let mut args = vec!["scratchpad", "status"];
    if let Some(name) = name {
        args.push(name);
    }
    instantwmctl::json(args)
}

fn get_scratchpad_info(name: &str) -> Result<Option<InstantWmScratchpadInfo>> {
    Ok(get_scratchpad_list(Some(name))?.into_iter().next())
}

fn is_scratchpad_registered(name: &str) -> Result<bool> {
    Ok(get_scratchpad_info(name)?.is_some())
}

pub fn reload_config() -> Result<()> {
    instantwmctl::run(["reload"])
}

pub fn set_mode(mode_name: &str) -> Result<()> {
    instantwmctl::run(["mode", "set", mode_name])
}

pub fn list_modes() -> Result<String> {
    let output = instantwmctl::output(["mode", "list"])?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
