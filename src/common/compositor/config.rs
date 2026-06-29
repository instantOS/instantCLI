//! Window manager configuration file manager
//!
//! This module provides utilities for managing shared Sway/i3/niri configuration
//! files that instantCLI uses for WM integration.

use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Marker for the integration block in the main config (sway/i3 use `#` comments).
const INTEGRATION_MARKER_START: &str = "# BEGIN instantCLI integration (managed automatically)";
const INTEGRATION_MARKER_END: &str = "# END instantCLI integration";

/// Marker for the integration block in niri's KDL config (KDL uses `//` comments).
const KDL_INTEGRATION_MARKER_START: &str =
    "// BEGIN instantCLI integration (managed automatically)";
const KDL_INTEGRATION_MARKER_END: &str = "// END instantCLI integration";

/// Header written into niri's `instant.kdl` the first time ins creates it.
///
/// Shared by the runtime read path (returns this when the file is missing so
/// subsequent upserts have a valid base) and the `setup niri` command (seeds
/// the file on disk). Kept here next to the KDL markers so all niri-specific
/// integration strings live in one place.
pub(crate) const INSTANT_KDL_HEADER: &str = "\
// instantCLI niri configuration
// This file is managed by instantCLI. Manual edits may be overwritten.
// It is loaded into your main niri config via an `include` directive.
";

/// Supported window managers
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowManager {
    Sway,
    I3,
    InstantWM,
    Niri,
}

impl WindowManager {
    /// Get the config directory name
    pub fn config_dir_name(&self) -> &'static str {
        match self {
            WindowManager::Sway => "sway",
            WindowManager::I3 => "i3",
            WindowManager::InstantWM => "instantwm",
            WindowManager::Niri => "niri",
        }
    }

    /// Get the display name
    pub fn name(&self) -> &'static str {
        match self {
            WindowManager::Sway => "Sway",
            WindowManager::I3 => "i3",
            WindowManager::InstantWM => "instantWM",
            WindowManager::Niri => "niri",
        }
    }

    /// Get the reload command
    pub fn reload_command(&self) -> &'static str {
        match self {
            WindowManager::Sway => "swaymsg",
            WindowManager::I3 => "i3-msg",
            WindowManager::InstantWM => "instantwmctl",
            WindowManager::Niri => "niri",
        }
    }

    /// Whether this WM supports cursor theme in config
    pub fn supports_cursor_theme(&self) -> bool {
        match self {
            WindowManager::Sway => true,
            WindowManager::I3 => false,
            WindowManager::InstantWM => false,
            WindowManager::Niri => false,
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
        let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));

        let (config_path, main_config_path) = match wm {
            WindowManager::InstantWM => {
                let dir = config_dir.join(wm.config_dir_name());
                (dir.join("assist.toml"), dir.join("config.toml"))
            }
            WindowManager::Niri => {
                let niri_config = env::var_os("NIRI_CONFIG").map(PathBuf::from);
                Self::resolve_niri_paths(niri_config.as_deref(), &config_dir)
            }
            _ => {
                let dir = config_dir.join(wm.config_dir_name());
                (dir.join("instant"), dir.join("config"))
            }
        };

        Self {
            wm,
            config_path,
            main_config_path,
        }
    }

    /// Resolve the (config_path, main_config_path) pair for niri.
    ///
    /// Mirrors niri's own resolution order for the main config, but skips
    /// `/etc/niri/config.kdl` because as a regular user we cannot write there.
    /// If `$NIRI_CONFIG` is set we use it as-is (so we patch the same file
    /// niri actually reads) and place `instant.kdl` as a sibling so the
    /// relative `include` line is portable.
    ///
    /// Returns `(instant.kdl path, main config path)`.
    fn resolve_niri_paths(
        niri_config_env: Option<&Path>,
        default_config_dir: &Path,
    ) -> (PathBuf, PathBuf) {
        let main = niri_config_env
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_config_dir.join("niri/config.kdl"));

        let instant = main
            .parent()
            .map(|p| p.join("instant.kdl"))
            .unwrap_or_else(|| default_config_dir.join("niri/instant.kdl"));

        (instant, main)
    }

    /// Get the path to the shared config file.
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    /// Get the window manager type.
    pub fn wm(&self) -> WindowManager {
        self.wm
    }

    /// Get the path to the main WM config file.
    pub fn main_config_path(&self) -> &PathBuf {
        &self.main_config_path
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

    /// Check if the config file is included in the main WM config.
    pub fn is_included_in_main_config(&self) -> Result<bool> {
        if !self.main_config_path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(&self.main_config_path)
            .with_context(|| format!("Failed to read {}", self.main_config_path.display()))?;

        let marker = match self.wm {
            WindowManager::Niri => KDL_INTEGRATION_MARKER_START,
            _ => INTEGRATION_MARKER_START,
        };
        Ok(content.contains(marker))
    }

    /// Ensure the config file is included in the main WM config.
    ///
    /// Returns `true` if the include was added, `false` if it already existed.
    pub fn ensure_included_in_main_config(&self) -> Result<bool> {
        if !self.main_config_path.exists() {
            // For niri, auto-create an empty main config so the include can be
            // added on first run. sway/i3/instantwm still require pre-existing
            // configs (matching their setup expectations).
            if self.wm == WindowManager::Niri {
                if let Some(parent) = self.main_config_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create directory {}", parent.display())
                    })?;
                }
                fs::write(&self.main_config_path, "").with_context(|| {
                    format!("Failed to create {}", self.main_config_path.display())
                })?;
            } else {
                anyhow::bail!(
                    "{} config not found at {}\nPlease ensure {} is installed and configured.",
                    self.wm.name(),
                    self.main_config_path.display(),
                    self.wm.name()
                );
            }
        }

        let content = fs::read_to_string(&self.main_config_path)
            .with_context(|| format!("Failed to read {}", self.main_config_path.display()))?;

        match self.wm {
            WindowManager::InstantWM => self.ensure_toml_include(&content),
            WindowManager::Niri => self.ensure_niri_include(&content),
            _ => self.ensure_sway_include(&content),
        }
    }

    fn ensure_toml_include(&self, content: &str) -> Result<bool> {
        let filename = self
            .config_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "assist.toml".to_string());

        if content.contains(&filename) {
            return Ok(false);
        }

        let include_entry = format!("\n[[includes]]\nfile = \"{}\"\n", filename);
        let new_content = format!("{}{}", content, include_entry);

        fs::write(&self.main_config_path, new_content)
            .with_context(|| format!("Failed to write {}", self.main_config_path.display()))?;

        Ok(true)
    }

    fn ensure_sway_include(&self, content: &str) -> Result<bool> {
        if content.contains(INTEGRATION_MARKER_START) {
            return Ok(false);
        }

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

    /// Append a KDL `include` directive to niri's main config, wrapped in markers.
    ///
    /// Niri supports `include "file.kdl"` at the top level since v25.11. We use a
    /// path relative to the main config so the include line stays portable. Since
    /// includes are positional and we append at the end, ins-managed settings
    /// override any duplicate keys the user may have written earlier in the main
    /// config.
    fn ensure_niri_include(&self, content: &str) -> Result<bool> {
        if content.contains(KDL_INTEGRATION_MARKER_START) {
            return Ok(false);
        }

        // Niri resolves relative include paths against the directory of the
        // including file, so a bare filename is the most portable form.
        let relative_path = self
            .config_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "instant.kdl".to_string());

        let include_line = format!("include \"{}\"", relative_path);
        // Add a leading newline if the file does not end with one, so the
        // block always starts on its own line.
        let separator = if !content.is_empty() && !content.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let integration_block = format!(
            "{}\n{}\n{}\n{}\n",
            separator, KDL_INTEGRATION_MARKER_START, include_line, KDL_INTEGRATION_MARKER_END
        );

        let new_content = format!("{}{}", content, integration_block);

        fs::write(&self.main_config_path, new_content)
            .with_context(|| format!("Failed to write {}", self.main_config_path.display()))?;

        Ok(true)
    }

    /// Reload WM configuration.
    pub fn reload(&self) -> Result<()> {
        match self.wm {
            WindowManager::InstantWM => crate::common::compositor::instantwm::reload_config(),
            WindowManager::Niri => crate::common::compositor::niri::reload_config(),
            _ => {
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
        }
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

        let (config_path, main_config_path) = match wm {
            WindowManager::Niri => (
                config_dir.join("instant.kdl"),
                config_dir.join("config.kdl"),
            ),
            WindowManager::InstantWM => (
                config_dir.join("assist.toml"),
                config_dir.join("config.toml"),
            ),
            _ => (config_dir.join("instant"), config_dir.join("config")),
        };

        WmConfigManager {
            wm,
            config_path,
            main_config_path,
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

    #[test]
    fn test_ensure_niri_include_appended_with_kdl_markers() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir, WindowManager::Niri);

        let mut main_config = fs::File::create(&manager.main_config_path).unwrap();
        writeln!(main_config, "// existing niri config").unwrap();
        writeln!(main_config, "input {{").unwrap();
        writeln!(main_config, "    mouse {{ accel-speed 0.1 }}").unwrap();
        writeln!(main_config, "}}").unwrap();

        // First call adds the include block.
        assert!(manager.ensure_included_in_main_config().unwrap());

        let content = fs::read_to_string(&manager.main_config_path).unwrap();
        assert!(content.contains(KDL_INTEGRATION_MARKER_START));
        assert!(content.contains(KDL_INTEGRATION_MARKER_END));
        assert!(content.contains("include \"instant.kdl\""));

        // Marker must come AFTER existing user content (positional override).
        let marker_idx = content.find(KDL_INTEGRATION_MARKER_START).unwrap();
        let user_idx = content.find("accel-speed 0.1").unwrap();
        assert!(marker_idx > user_idx);

        // Second call is a no-op.
        assert!(!manager.ensure_included_in_main_config().unwrap());

        // is_included_in_main_config also recognises the KDL marker.
        assert!(manager.is_included_in_main_config().unwrap());
    }

    #[test]
    fn test_ensure_niri_include_creates_missing_main_config() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_test_manager(&temp_dir, WindowManager::Niri);

        // No main config yet — niri should bootstrap one.
        assert!(!manager.main_config_path.exists());
        assert!(manager.ensure_included_in_main_config().unwrap());

        let content = fs::read_to_string(&manager.main_config_path).unwrap();
        assert!(content.contains("include \"instant.kdl\""));
        assert!(content.contains(KDL_INTEGRATION_MARKER_START));
    }

    #[test]
    fn test_resolve_niri_paths_default() {
        let default = PathBuf::from("/home/user/.config");
        let (instant, main) = WmConfigManager::resolve_niri_paths(None, &default);
        assert_eq!(main, PathBuf::from("/home/user/.config/niri/config.kdl"));
        assert_eq!(
            instant,
            PathBuf::from("/home/user/.config/niri/instant.kdl")
        );
    }

    #[test]
    fn test_resolve_niri_paths_with_niri_config() {
        let default = PathBuf::from("/home/user/.config");
        let custom = PathBuf::from("/home/user/dotfiles/niri.kdl");
        let (instant, main) = WmConfigManager::resolve_niri_paths(Some(&custom), &default);
        assert_eq!(main, custom);
        // instant.kdl is a sibling of the main config so the relative
        // `include` line resolves correctly.
        assert_eq!(instant, PathBuf::from("/home/user/dotfiles/instant.kdl"));
    }

    #[test]
    fn test_resolve_niri_paths_empty_env_var_treated_as_unset() {
        let default = PathBuf::from("/home/user/.config");
        let empty = PathBuf::new();
        let (instant, main) = WmConfigManager::resolve_niri_paths(Some(&empty), &default);
        assert_eq!(main, PathBuf::from("/home/user/.config/niri/config.kdl"));
        assert_eq!(
            instant,
            PathBuf::from("/home/user/.config/niri/instant.kdl")
        );
    }

    #[test]
    fn test_resolve_niri_paths_relative_env_var_preserved() {
        let default = PathBuf::from("/home/user/.config");
        let relative = PathBuf::from("niri/config.kdl");
        let (instant, main) = WmConfigManager::resolve_niri_paths(Some(&relative), &default);
        // Niri resolves relative paths against cwd; we mirror that exactly
        // so both processes look at the same files.
        assert_eq!(main, relative);
        assert_eq!(instant, PathBuf::from("niri/instant.kdl"));
    }
}
