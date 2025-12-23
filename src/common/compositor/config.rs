//! Sway configuration file manager
//!
//! This module provides utilities for managing a shared Sway configuration file
//! that multiple components of instantCLI can write to. The file uses marker-based
//! sections to allow independent updates of different configuration areas.

use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Marker prefix for section start
const SECTION_START_PREFIX: &str = "# --- BEGIN ";
/// Marker suffix for section start
const SECTION_START_SUFFIX: &str = " ---";
/// Marker prefix for section end
const SECTION_END_PREFIX: &str = "# --- END ";
/// Marker suffix for section end
const SECTION_END_SUFFIX: &str = " ---";

/// Marker for the integration block in the main sway config
const INTEGRATION_MARKER_START: &str = "# BEGIN instantCLI integration (managed automatically)";
const INTEGRATION_MARKER_END: &str = "# END instantCLI integration";

/// Manager for the shared Sway configuration file.
///
/// This manages the `~/.config/sway/instant` file which is included from the
/// main sway config. Multiple components can write to different sections of
/// this file without interfering with each other.
pub struct SwayConfigManager {
    /// Path to the shared instant config file
    config_path: PathBuf,
    /// Path to the main sway config file
    main_config_path: PathBuf,
}

impl Default for SwayConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SwayConfigManager {
    /// Create a new SwayConfigManager with default paths.
    pub fn new() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sway");

        Self {
            config_path: config_dir.join("instant"),
            main_config_path: config_dir.join("config"),
        }
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

    /// Write the config file contents.
    fn write_config(&self, content: &str) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        fs::write(&self.config_path, content)
            .with_context(|| format!("Failed to write {}", self.config_path.display()))
    }

    /// Write or update a section in the config file.
    ///
    /// The section is identified by name and wrapped in markers:
    /// ```text
    /// # --- BEGIN section_name ---
    /// <content>
    /// # --- END section_name ---
    /// ```
    ///
    /// If the section already exists, it is replaced. Otherwise, it is appended.
    pub fn write_section(&self, section_name: &str, content: &str) -> Result<()> {
        let current = self.read_config()?;

        let start_marker = format!(
            "{}{}{}",
            SECTION_START_PREFIX, section_name, SECTION_START_SUFFIX
        );
        let end_marker = format!(
            "{}{}{}",
            SECTION_END_PREFIX, section_name, SECTION_END_SUFFIX
        );

        // Build the new section
        let trimmed_content = content.trim();
        let new_section = format!("{}\n{}\n{}\n", start_marker, trimmed_content, end_marker);

        let new_content = if current.contains(&start_marker) {
            // Replace existing section
            let mut result = String::new();
            let mut in_section = false;
            let mut replaced = false;

            for line in current.lines() {
                if line.trim() == start_marker.trim() {
                    in_section = true;
                    if !replaced {
                        result.push_str(&new_section);
                        replaced = true;
                    }
                    continue;
                }

                if line.trim() == end_marker.trim() {
                    in_section = false;
                    continue;
                }

                if !in_section {
                    result.push_str(line);
                    result.push('\n');
                }
            }

            result
        } else {
            // Append new section
            let mut result = current;
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&new_section);
            result
        };

        // Ensure file starts with header comment
        let final_content = if !new_content.starts_with("# instantCLI sway configuration") {
            format!(
                "# instantCLI sway configuration\n# This file is managed by instantCLI. Manual edits may be overwritten.\n\n{}",
                new_content
            )
        } else {
            new_content
        };

        self.write_config(&final_content)
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

    /// Ensure the config file is included in the main sway config.
    ///
    /// Returns `true` if the include was added, `false` if it already existed.
    pub fn ensure_included_in_main_config(&self) -> Result<bool> {
        if !self.main_config_path.exists() {
            anyhow::bail!(
                "Sway config not found at {}\nPlease ensure Sway is installed and configured.",
                self.main_config_path.display()
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

    /// Reload Sway configuration.
    pub fn reload(&self) -> Result<()> {
        let status = std::process::Command::new("swaymsg")
            .arg("reload")
            .status()
            .context("Failed to run swaymsg reload")?;

        if !status.success() {
            anyhow::bail!("swaymsg reload returned non-zero exit code");
        }

        Ok(())
    }

    /// Reload Sway configuration only if the config has changed.
    ///
    /// Pass the hash from before making changes. If the current hash differs,
    /// Sway will be reloaded.
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

    fn create_test_manager(temp_dir: &TempDir) -> SwayConfigManager {
        let config_dir = temp_dir.path().join("sway");
        fs::create_dir_all(&config_dir).unwrap();

        SwayConfigManager {
            config_path: config_dir.join("instant"),
            main_config_path: config_dir.join("config"),
        }
    }

    #[test]
    fn test_write_section_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir);

        manager.write_section("test", "content here").unwrap();

        let content = fs::read_to_string(&manager.config_path).unwrap();
        assert!(content.contains("# --- BEGIN test ---"));
        assert!(content.contains("content here"));
        assert!(content.contains("# --- END test ---"));
    }

    #[test]
    fn test_write_section_replace() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir);

        manager.write_section("test", "original").unwrap();
        manager.write_section("test", "updated").unwrap();

        let content = fs::read_to_string(&manager.config_path).unwrap();
        assert!(content.contains("updated"));
        assert!(!content.contains("original"));
        // Should only have one section
        assert_eq!(content.matches("# --- BEGIN test ---").count(), 1);
    }

    #[test]
    fn test_write_multiple_sections() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir);

        manager.write_section("section1", "content1").unwrap();
        manager.write_section("section2", "content2").unwrap();

        let content = fs::read_to_string(&manager.config_path).unwrap();
        assert!(content.contains("# --- BEGIN section1 ---"));
        assert!(content.contains("content1"));
        assert!(content.contains("# --- BEGIN section2 ---"));
        assert!(content.contains("content2"));
    }

    #[test]
    fn test_ensure_included_in_main_config() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir);

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
