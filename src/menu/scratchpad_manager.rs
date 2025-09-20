use crate::common::compositor::CompositorType;
use crate::scratchpad::{config::ScratchpadConfig, hide_scratchpad, show_scratchpad};
use anyhow::{Context, Result};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Manages scratchpad visibility for the menu server
pub struct ScratchpadManager {
    compositor: CompositorType,
    config: ScratchpadConfig,
    visible: Arc<AtomicBool>,
}

impl ScratchpadManager {
    /// Create a new scratchpad manager
    pub fn new(compositor: CompositorType, config: ScratchpadConfig) -> Self {
        Self {
            compositor,
            config,
            visible: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Show the scratchpad if not already visible
    pub fn show(&self) -> Result<()> {
        // Check if already visible to avoid unnecessary operations
        if !self.visible.load(Ordering::SeqCst) {
            show_scratchpad(&self.compositor, &self.config)
                .context("Failed to show menu server scratchpad")?;
            self.visible.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    /// Show the scratchpad without checks - optimized for performance
    /// Use this when you know the scratchpad should be shown regardless of current state
    pub fn show_fast(&self) -> Result<()> {
        match &self.compositor {
            CompositorType::Sway => {
                let window_class = self.config.window_class();
                crate::common::compositor::sway::show_scratchpad(&window_class)
                    .context("Failed to show scratchpad in Sway")?;
            }
            CompositorType::Hyprland => {
                let workspace_name = self.config.workspace_name();
                crate::common::compositor::hyprland::show_special_workspace(&workspace_name)
                    .context("Failed to show special workspace in Hyprland")?;
            }
            CompositorType::Other(_) => {
                // For unsupported compositors, fall back to normal show
                show_scratchpad(&self.compositor, &self.config)
                    .context("Failed to show menu server scratchpad")?;
            }
        }
        self.visible.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Hide the scratchpad if currently visible
    pub fn hide(&self) -> Result<()> {
        // Check if currently visible to avoid unnecessary operations
        if self.visible.load(Ordering::SeqCst) {
            hide_scratchpad(&self.compositor, &self.config)
                .context("Failed to hide menu server scratchpad")?;
            self.visible.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    /// Hide the scratchpad without checks - optimized for performance
    /// Use this when you need the absolute fastest hide operation
    pub fn hide_fast(&self) -> Result<()> {
        match &self.compositor {
            CompositorType::Sway => {
                let window_class = self.config.window_class();
                crate::common::compositor::sway::hide_scratchpad(&window_class)
                    .context("Failed to hide scratchpad in Sway")?;
            }
            CompositorType::Hyprland => {
                let workspace_name = self.config.workspace_name();
                crate::common::compositor::hyprland::hide_special_workspace(&workspace_name)
                    .context("Failed to hide special workspace in Hyprland")?;
            }
            CompositorType::Other(_) => {
                // For unsupported compositors, fall back to normal hide
                hide_scratchpad(&self.compositor, &self.config)
                    .context("Failed to hide menu server scratchpad")?;
            }
        }
        self.visible.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Mark the scratchpad as visible without actually showing it
    /// (useful when the scratchpad is initially visible)
    pub fn mark_visible(&self) {
        self.visible.store(true, Ordering::SeqCst);
    }

    /// Get the scratchpad configuration
    pub fn config(&self) -> &ScratchpadConfig {
        &self.config
    }

    /// Get the compositor type
    pub fn compositor(&self) -> &CompositorType {
        &self.compositor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratchpad_manager_creation() {
        let compositor = CompositorType::Other("test".to_string());
        let config = ScratchpadConfig::new("test".to_string());
        let manager = ScratchpadManager::new(compositor, config);

        assert_eq!(manager.config().name, "test");
    }
}
