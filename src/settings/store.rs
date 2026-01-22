use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::common::paths;

/// Represents the hierarchical settings structure.
/// Settings are organized into nested tables based on their dotted keys.
/// For example, "appearance.animations" becomes appearance.animations in TOML.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SettingsFile {
    #[serde(flatten)]
    sections: BTreeMap<String, toml::Value>,
}

impl SettingsFile {
    /// Get a value by its dotted key path (e.g., "appearance.animations")
    fn get(&self, key: &str) -> Option<&toml::Value> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() {
            return None;
        }

        let mut current = self.sections.get(parts[0])?;

        for &part in &parts[1..] {
            current = current.get(part)?;
        }

        Some(current)
    }

    /// Set a value by its dotted key path (e.g., "appearance.animations")
    fn set(&mut self, key: &str, value: toml::Value) {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() {
            return;
        }

        // For a single-part key, just set it directly
        if parts.len() == 1 {
            self.sections.insert(parts[0].to_string(), value);
            return;
        }

        // For multi-part keys, navigate/create the nested structure
        let section_name = parts[0];
        let section = self
            .sections
            .entry(section_name.to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));

        let mut current = section;
        for &part in &parts[1..parts.len() - 1] {
            let table = current
                .as_table_mut()
                .expect("expected table in settings hierarchy");
            current = table
                .entry(part.to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        }

        let final_key = parts[parts.len() - 1];
        if let Some(table) = current.as_table_mut() {
            table.insert(final_key.to_string(), value);
        }
    }

    /// Remove a value by its dotted key path
    fn remove(&mut self, key: &str) {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() {
            return;
        }

        if parts.len() == 1 {
            self.sections.remove(parts[0]);
            return;
        }

        // Navigate to parent and remove the final key
        let mut current = match self.sections.get_mut(parts[0]) {
            Some(v) => v,
            None => return,
        };

        for &part in &parts[1..parts.len() - 1] {
            current = match current.as_table_mut().and_then(|t| t.get_mut(part)) {
                Some(v) => v,
                None => return,
            };
        }

        let final_key = parts[parts.len() - 1];
        if let Some(table) = current.as_table_mut() {
            table.remove(final_key);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BoolSettingKey {
    pub key: &'static str,
    pub default: bool,
}

impl BoolSettingKey {
    pub const fn new(key: &'static str, default: bool) -> Self {
        Self { key, default }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StringSettingKey {
    pub key: &'static str,
    pub default: &'static str,
}

impl StringSettingKey {
    pub const fn new(key: &'static str, default: &'static str) -> Self {
        Self { key, default }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IntSettingKey {
    pub key: &'static str,
    pub default: i64,
}

impl IntSettingKey {
    pub const fn new(key: &'static str, default: i64) -> Self {
        Self { key, default }
    }
}

/// Key for optional string settings (no default value - may be unset)
#[derive(Debug, Clone, Copy)]
pub struct OptionalStringSettingKey {
    pub key: &'static str,
}

impl OptionalStringSettingKey {
    pub const fn new(key: &'static str) -> Self {
        Self { key }
    }
}

// Wallpaper setting keys
pub const WALLPAPER_PATH_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("appearance.wallpaper_path");
pub const WALLPAPER_LOGO_KEY: BoolSettingKey =
    BoolSettingKey::new("appearance.wallpaper_logo", true);
pub const WALLPAPER_BG_COLOR_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("appearance.wallpaper_bg_color");
pub const WALLPAPER_FG_COLOR_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("appearance.wallpaper_fg_color");

// GTK appearance setting keys
pub const GTK_THEME_KEY: StringSettingKey =
    StringSettingKey::new("appearance.gtk_theme", "Unknown");
pub const GTK_ICON_THEME_KEY: StringSettingKey =
    StringSettingKey::new("appearance.gtk_icon_theme", "Unknown");

// System setting keys
pub const PACMAN_AUTOCLEAN_KEY: BoolSettingKey =
    BoolSettingKey::new("system.pacman_autoclean", false);

#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    data: SettingsFile,
}

impl SettingsStore {
    pub fn load() -> Result<Self> {
        let path = settings_file_path()?;
        Self::load_from_path(path)
    }

    pub fn load_from_path(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                path,
                data: SettingsFile::default(),
            });
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("reading settings file from {}", path.display()))?;
        let data = toml::from_str(&contents)
            .with_context(|| format!("parsing settings file at {}", path.display()))?;

        Ok(Self { path, data })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating settings directory at {}", parent.display()))?;
        }

        let contents =
            toml::to_string_pretty(&self.data).context("serializing settings to toml")?;
        fs::write(&self.path, contents)
            .with_context(|| format!("writing settings file to {}", self.path.display()))?;
        Ok(())
    }

    pub fn bool(&self, key: BoolSettingKey) -> bool {
        self.data
            .get(key.key)
            .and_then(|value| value.as_bool())
            .unwrap_or(key.default)
    }

    pub fn set_bool(&mut self, key: BoolSettingKey, value: bool) {
        self.data.set(key.key, toml::Value::Boolean(value));
    }

    pub fn string(&self, key: StringSettingKey) -> String {
        self.data
            .get(key.key)
            .and_then(|value| value.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| key.default.to_string())
    }

    pub fn set_string<S: Into<String>>(&mut self, key: StringSettingKey, value: S) {
        self.data.set(key.key, toml::Value::String(value.into()));
    }

    pub fn int(&self, key: IntSettingKey) -> i64 {
        self.data
            .get(key.key)
            .and_then(|value| value.as_integer())
            .unwrap_or(key.default)
    }

    pub fn set_int(&mut self, key: IntSettingKey, value: i64) {
        self.data.set(key.key, toml::Value::Integer(value));
    }

    pub fn optional_string(&self, key: OptionalStringSettingKey) -> Option<String> {
        self.data
            .get(key.key)
            .and_then(|value| value.as_str())
            .map(|s| s.to_string())
    }

    pub fn set_optional_string<S: Into<String>>(
        &mut self,
        key: OptionalStringSettingKey,
        value: Option<S>,
    ) {
        match value {
            Some(v) => self.data.set(key.key, toml::Value::String(v.into())),
            None => self.data.remove(key.key),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn contains(&self, key: &str) -> bool {
        self.data.get(key).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.data.sections.is_empty()
    }
}

fn settings_file_path() -> Result<PathBuf> {
    Ok(paths::instant_config_dir()?.join("settings.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_hierarchical_serialization() {
        let mut settings = SettingsFile::default();

        // Set some values with dotted keys
        settings.set("appearance.animations", toml::Value::Boolean(true));
        settings.set("desktop.layout", toml::Value::String("tile".to_string()));
        settings.set("printers.services", toml::Value::Boolean(false));
        settings.set("desktop.clipboard.enabled", toml::Value::Boolean(true));

        // Serialize to TOML
        let toml_str = toml::to_string_pretty(&settings).unwrap();

        // Verify it creates proper hierarchical structure
        assert!(toml_str.contains("[appearance]"));
        assert!(toml_str.contains("animations = true"));
        assert!(toml_str.contains("[desktop]"));
        assert!(toml_str.contains("layout = \"tile\""));
        assert!(toml_str.contains("[printers]"));
        assert!(toml_str.contains("services = false"));

        // Verify nested structure
        assert!(toml_str.contains("[desktop.clipboard]") || toml_str.contains("[desktop]\n"));

        // Should NOT contain the old flat format with quoted keys
        assert!(!toml_str.contains("\"appearance.animations\""));
        assert!(!toml_str.contains("[values]"));
    }

    #[test]
    fn test_get_and_set() {
        let mut settings = SettingsFile::default();

        // Test setting and getting boolean
        settings.set("appearance.animations", toml::Value::Boolean(true));
        let value = settings.get("appearance.animations").unwrap();
        assert_eq!(value.as_bool(), Some(true));

        // Test setting and getting string
        settings.set("desktop.layout", toml::Value::String("grid".to_string()));
        let value = settings.get("desktop.layout").unwrap();
        assert_eq!(value.as_str(), Some("grid"));

        // Test deeply nested value
        settings.set("a.b.c.d", toml::Value::Integer(42));
        let value = settings.get("a.b.c.d").unwrap();
        assert_eq!(value.as_integer(), Some(42));

        // Test non-existent key
        assert!(settings.get("nonexistent.key").is_none());
    }

    #[test]
    fn test_settings_store_bool() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create a new store
        let mut store = SettingsStore::load_from_path(path.clone()).unwrap();

        let key = BoolSettingKey::new("appearance.animations", false);

        // Test default value
        assert!(!store.bool(key));

        // Set and verify
        store.set_bool(key, true);
        assert!(store.bool(key));

        // Save and reload
        store.save().unwrap();
        let reloaded = SettingsStore::load_from_path(path).unwrap();
        assert!(reloaded.bool(key));
    }

    #[test]
    fn test_settings_store_string() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let mut store = SettingsStore::load_from_path(path.clone()).unwrap();

        let key = StringSettingKey::new("desktop.layout", "tile");

        // Test default value
        assert_eq!(store.string(key), "tile");

        // Set and verify
        store.set_string(key, "grid");
        assert_eq!(store.string(key), "grid");

        // Save and reload
        store.save().unwrap();
        let reloaded = SettingsStore::load_from_path(path).unwrap();
        assert_eq!(reloaded.string(key), "grid");
    }

    #[test]
    fn test_settings_store_int() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let mut store = SettingsStore::load_from_path(path.clone()).unwrap();

        let key = IntSettingKey::new("desktop.mouse.sensitivity", 50);

        // Test default value
        assert_eq!(store.int(key), 50);

        // Set and verify
        store.set_int(key, 75);
        assert_eq!(store.int(key), 75);

        // Save and reload
        store.save().unwrap();
        let reloaded = SettingsStore::load_from_path(path).unwrap();
        assert_eq!(reloaded.int(key), 75);
    }

    #[test]
    fn test_load_and_save_hierarchical_format() {
        // Create a temp file with new hierarchical format
        let temp_file = NamedTempFile::new().unwrap();
        let hierarchical_content = r#"
[appearance]
animations = true

[desktop]
layout = "monocle"
"#;
        fs::write(temp_file.path(), hierarchical_content).unwrap();

        let path = temp_file.path().to_path_buf();

        // Load the hierarchical format
        let store = SettingsStore::load_from_path(path.clone()).unwrap();

        let bool_key = BoolSettingKey::new("appearance.animations", false);
        let string_key = StringSettingKey::new("desktop.layout", "tile");

        assert!(store.bool(bool_key));
        assert_eq!(store.string(string_key), "monocle");

        // Save should maintain hierarchical format
        store.save().unwrap();

        let saved_content = fs::read_to_string(&path).unwrap();
        assert!(saved_content.contains("[appearance]"));
        assert!(saved_content.contains("[desktop]"));
        assert!(!saved_content.contains("[values]"));
        assert!(!saved_content.contains("\"appearance.animations\""));
    }

    #[test]
    fn test_empty_store() {
        let store = SettingsStore {
            path: PathBuf::from("/tmp/test"),
            data: SettingsFile::default(),
        };

        assert!(store.is_empty());

        let key = BoolSettingKey::new("test.key", true);
        assert!(store.bool(key)); // Should return default
    }
}
