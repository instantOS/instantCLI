//! Window manager configuration file manager
//!
//! This module provides utilities for managing shared Sway/i3 configuration files
//! that instantCLI uses for WM integration.

use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Marker for the integration block in the main config
const INTEGRATION_MARKER_START: &str = "# BEGIN instantCLI integration (managed automatically)";
const INTEGRATION_MARKER_END: &str = "# END instantCLI integration";

/// Supported window managers
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowManager {
    Sway,
    I3,
}

impl WindowManager {
    /// Get the config directory name
    pub fn config_dir_name(&self) -> &'static str {
        match self {
            WindowManager::Sway => "sway",
            WindowManager::I3 => "i3",
        }
    }

    /// Get the display name
    pub fn name(&self) -> &'static str {
        match self {
            WindowManager::Sway => "Sway",
            WindowManager::I3 => "i3",
        }
    }

    /// Get the reload command
    pub fn reload_command(&self) -> &'static str {
        match self {
            WindowManager::Sway => "swaymsg",
            WindowManager::I3 => "i3-msg",
        }
    }

    /// Whether this WM supports cursor theme in config
    pub fn supports_cursor_theme(&self) -> bool {
        match self {
            WindowManager::Sway => true,
            WindowManager::I3 => false, // X11 uses XCURSOR_THEME env var
        }
    }
}

/// Manager for the shared WM configuration file.
///
/// This manages the `~/.config/{sway,i3}/instant` file which is included from the
/// main WM config.
pub struct WmConfigManager {
    /// Which window manager
    wm: WindowManager,
    /// Path to the shared instant config file
    config_path: PathBuf,
    /// Path to the main WM config file
    main_config_path: PathBuf,
}

impl WmConfigManager {
    /// Create a new WmConfigManager for the given window manager.
    pub fn new(wm: WindowManager) -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join(wm.config_dir_name());

        Self {
            wm,
            config_path: config_dir.join("instant"),
            main_config_path: config_dir.join("config"),
        }
    }

    /// Get the window manager type.
    pub fn wm(&self) -> WindowManager {
        self.wm
    }

    /// Get the path to the shared config file.
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    /// Compute hash of the config file for change detection.
    pub fn hash_config(&self) -> Result<u64> {
        if !self.config_path.exists() {
            return Ok(0);
        }
        let content = fs::read(&self.config_path)
            .with_context(|| format!("Failed to read {}", self.config_path.display()))?;
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        Ok(hasher.finish())
    }

    /// Read the current config file contents.
    fn read_config(&self) -> Result<String> {
        if !self.config_path.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(&self.config_path)
            .with_context(|| format!("Failed to read {}", self.config_path.display()))
    }

    /// Write the full config file contents, replacing any existing content.
    pub fn write_full_config(&self, content: &str) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        fs::write(&self.config_path, content)
            .with_context(|| format!("Failed to write {}", self.config_path.display()))
    }

    /// Check if the config file is included in the main sway config.
    pub fn is_included_in_main_config(&self) -> Result<bool> {
        if !self.main_config_path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(&self.main_config_path)
            .with_context(|| format!("Failed to read {}", self.main_config_path.display()))?;

        Ok(content.contains(INTEGRATION_MARKER_START))
    }

    /// Ensure the config file is included in the main WM config.
    ///
    /// Returns `true` if the include was added, `false` if it already existed.
    pub fn ensure_included_in_main_config(&self) -> Result<bool> {
        if !self.main_config_path.exists() {
            anyhow::bail!(
                "{} config not found at {}\nPlease ensure {} is installed and configured.",
                self.wm.name(),
                self.main_config_path.display(),
                self.wm.name()
            );
        }

        let content = fs::read_to_string(&self.main_config_path)
            .with_context(|| format!("Failed to read {}", self.main_config_path.display()))?;

        if content.contains(INTEGRATION_MARKER_START) {
            return Ok(false);
        }

        // Build include line with tilde notation
        let home_dir = dirs::home_dir().context("Unable to determine home directory")?;
        let relative_path = self
            .config_path
            .strip_prefix(&home_dir)
            .ok()
            .map(|p| format!("~/{}", p.display()))
            .unwrap_or_else(|| self.config_path.display().to_string());

        let include_line = format!("include {}", relative_path);
        let integration_block = format!(
            "\n{}\n{}\n{}\n",
            INTEGRATION_MARKER_START, include_line, INTEGRATION_MARKER_END
        );

        let new_content = format!("{}{}", content, integration_block);

        fs::write(&self.main_config_path, new_content)
            .with_context(|| format!("Failed to write {}", self.main_config_path.display()))?;

        Ok(true)
    }

    /// Reload WM configuration.
    pub fn reload(&self) -> Result<()> {
        let cmd = self.wm.reload_command();
        let status = std::process::Command::new(cmd)
            .arg("reload")
            .status()
            .with_context(|| format!("Failed to run {} reload", cmd))?;

        if !status.success() {
            anyhow::bail!("{} reload returned non-zero exit code", cmd);
        }

        Ok(())
    }

    /// Reload WM configuration only if the config has changed.
    ///
    /// Pass the hash from before making changes. If the current hash differs,
    /// WM will be reloaded.
    pub fn reload_if_changed(&self, initial_hash: u64) -> Result<bool> {
        let current_hash = self.hash_config()?;
        if current_hash != initial_hash {
            self.reload()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_manager(temp_dir: &TempDir, wm: WindowManager) -> WmConfigManager {
        let config_dir = temp_dir.path().join(wm.config_dir_name());
        fs::create_dir_all(&config_dir).unwrap();

        WmConfigManager {
            wm,
            config_path: config_dir.join("instant"),
            main_config_path: config_dir.join("config"),
        }
    }

    #[test]
    fn test_write_full_config() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir, WindowManager::Sway);

        manager.write_full_config("test content").unwrap();

        let content = fs::read_to_string(&manager.config_path).unwrap();
        assert_eq!(content, "test content");
    }

    #[test]
    fn test_hash_config() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir, WindowManager::I3);

        // Non-existent file returns 0
        assert_eq!(manager.hash_config().unwrap(), 0);

        // Write content and verify hash changes
        manager.write_full_config("content").unwrap();
        let hash1 = manager.hash_config().unwrap();
        assert_ne!(hash1, 0);

        // Same content = same hash
        let hash2 = manager.hash_config().unwrap();
        assert_eq!(hash1, hash2);

        // Different content = different hash
        manager.write_full_config("different").unwrap();
        let hash3 = manager.hash_config().unwrap();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_ensure_included_in_main_config() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir, WindowManager::Sway);

        // Create main config
        let mut main_config = fs::File::create(&manager.main_config_path).unwrap();
        writeln!(main_config, "# Sway config").unwrap();

        // First call should add include
        assert!(manager.ensure_included_in_main_config().unwrap());

        // Second call should not add again
        assert!(!manager.ensure_included_in_main_config().unwrap());

        let content = fs::read_to_string(&manager.main_config_path).unwrap();
        assert!(content.contains(INTEGRATION_MARKER_START));
        assert!(content.contains("include"));
    }
}
