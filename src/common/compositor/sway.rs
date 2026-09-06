use super::ipc_tree::{self, SWAY_CONFIG};
use super::{ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::Result;

pub struct Sway;

impl ScratchpadProvider for Sway {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        if !self.is_window_running(config)? {
            ipc_tree::create_and_wait(&SWAY_CONFIG, config)?;
        }
        ipc_tree::show_scratchpad(&SWAY_CONFIG, &config.window_class())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        ipc_tree::hide_scratchpad(&SWAY_CONFIG, &config.window_class())
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        if self.is_window_running(config)? {
            ipc_tree::toggle_scratchpad(&SWAY_CONFIG, &window_class)?;
        } else {
            ipc_tree::create_and_wait(&SWAY_CONFIG, config)?;
            ipc_tree::show_scratchpad(&SWAY_CONFIG, &window_class)?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        ipc_tree::get_all_scratchpad_windows(&SWAY_CONFIG)
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        ipc_tree::window_exists(&SWAY_CONFIG, &config.window_class())
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        ipc_tree::is_window_visible(&SWAY_CONFIG, &config.window_class())
    }

    fn show_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        ipc_tree::show_scratchpad(&SWAY_CONFIG, &config.window_class())
    }

    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        ipc_tree::hide_scratchpad(&SWAY_CONFIG, &config.window_class())
    }

    fn supports_scratchpad(&self) -> bool {
        true
    }
}

/// Execute swaymsg command (used by settings and assist modules).
pub fn swaymsg(command: &str) -> Result<String> {
    ipc_tree::msg(&SWAY_CONFIG, command)
}

/// Execute swaymsg -t get_tree (used by settings and assist modules).
pub fn swaymsg_get_tree() -> Result<String> {
    ipc_tree::get_tree(&SWAY_CONFIG)
}
