use super::{ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::Result;

pub struct Fallback;

impl ScratchpadProvider for Fallback {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        // For fallback, show just spawns the terminal
        super::create_terminal_process(config)
    }

    fn hide(&self, _config: &ScratchpadConfig) -> Result<()> {
        // Cannot hide in fallback mode (no WM control)
        Ok(())
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        // Toggle just shows (spawns) in fallback mode
        self.show(config)
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        Ok(Vec::new())
    }

    fn is_window_running(&self, _config: &ScratchpadConfig) -> Result<bool> {
        // Cannot reliably detect window in fallback mode
        Ok(false)
    }

    fn is_visible(&self, _config: &ScratchpadConfig) -> Result<bool> {
        // Cannot reliably detect visibility in fallback mode
        Ok(false)
    }
}
