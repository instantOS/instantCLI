use super::ipc_tree::{self, I3_CONFIG};
use super::{ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::Result;

pub struct I3;

impl ScratchpadProvider for I3 {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        if !self.is_window_running(config)? {
            ipc_tree::create_and_wait(&I3_CONFIG, config)?;
        }
        ipc_tree::show_scratchpad(&I3_CONFIG, &config.window_class())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        ipc_tree::hide_scratchpad(&I3_CONFIG, &config.window_class())
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        if self.is_window_running(config)? {
            ipc_tree::toggle_scratchpad(&I3_CONFIG, &window_class)?;
        } else {
            ipc_tree::create_and_wait(&I3_CONFIG, config)?;
            ipc_tree::show_scratchpad(&I3_CONFIG, &window_class)?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        ipc_tree::get_all_scratchpad_windows(&I3_CONFIG)
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        ipc_tree::window_exists(&I3_CONFIG, &config.window_class())
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        ipc_tree::is_window_visible(&I3_CONFIG, &config.window_class())
    }

    fn show_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        ipc_tree::show_scratchpad(&I3_CONFIG, &config.window_class())
    }

    fn hide_unchecked(&self, config: &ScratchpadConfig) -> Result<()> {
        ipc_tree::hide_scratchpad(&I3_CONFIG, &config.window_class())
    }

    fn supports_scratchpad(&self) -> bool {
        true
    }
}
