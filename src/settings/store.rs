use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Represents the hierarchical settings structure.
/// Settings are organized into nested tables based on their dotted keys.
/// For example, "appearance.autotheming" becomes appearance.autotheming in TOML.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SettingsFile {
    #[serde(flatten)]
    sections: BTreeMap<String, toml::Value>,
}

impl SettingsFile {
    /// Get a value by its dotted key path (e.g., "appearance.autotheming")
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

    /// Set a value by its dotted key path (e.g., "appearance.autotheming")
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

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_empty(&self) -> bool {
        self.data.sections.is_empty()
    }
}

fn settings_file_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("unable to determine user config directory")?
        .join("instant");

    fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating config directory at {}", config_dir.display()))?;

    Ok(config_dir.join("settings.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_hierarchical_serialization() {
        let mut settings = SettingsFile::default();

        // Set some values with dotted keys
        settings.set("appearance.autotheming", toml::Value::Boolean(true));
        settings.set("workspace.layout", toml::Value::String("tile".to_string()));
        settings.set("printers.services", toml::Value::Boolean(false));
        settings.set("desktop.clipboard.enabled", toml::Value::Boolean(true));

        // Serialize to TOML
        let toml_str = toml::to_string_pretty(&settings).unwrap();

        // Verify it creates proper hierarchical structure
        assert!(toml_str.contains("[appearance]"));
        assert!(toml_str.contains("autotheming = true"));
        assert!(toml_str.contains("[workspace]"));
        assert!(toml_str.contains("layout = \"tile\""));
        assert!(toml_str.contains("[printers]"));
        assert!(toml_str.contains("services = false"));

        // Verify nested structure
        assert!(toml_str.contains("[desktop.clipboard]") || toml_str.contains("[desktop]\n"));

        // Should NOT contain the old flat format with quoted keys
        assert!(!toml_str.contains("\"appearance.autotheming\""));
        assert!(!toml_str.contains("[values]"));
    }

    #[test]
    fn test_get_and_set() {
        let mut settings = SettingsFile::default();

        // Test setting and getting boolean
        settings.set("appearance.autotheming", toml::Value::Boolean(true));
        let value = settings.get("appearance.autotheming").unwrap();
        assert_eq!(value.as_bool(), Some(true));

        // Test setting and getting string
        settings.set("workspace.layout", toml::Value::String("grid".to_string()));
        let value = settings.get("workspace.layout").unwrap();
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
        let mut temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create a new store
        let mut store = SettingsStore::load_from_path(path.clone()).unwrap();

        let key = BoolSettingKey::new("appearance.autotheming", false);

        // Test default value
        assert_eq!(store.bool(key), false);

        // Set and verify
        store.set_bool(key, true);
        assert_eq!(store.bool(key), true);

        // Save and reload
        store.save().unwrap();
        let reloaded = SettingsStore::load_from_path(path).unwrap();
        assert_eq!(reloaded.bool(key), true);
    }

    #[test]
    fn test_settings_store_string() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let mut store = SettingsStore::load_from_path(path.clone()).unwrap();

        let key = StringSettingKey::new("workspace.layout", "tile");

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
    fn test_load_and_save_hierarchical_format() {
        // Create a temp file with new hierarchical format
        let temp_file = NamedTempFile::new().unwrap();
        let hierarchical_content = r#"
[appearance]
autotheming = true

[workspace]
layout = "monocle"
"#;
        fs::write(temp_file.path(), hierarchical_content).unwrap();

        let path = temp_file.path().to_path_buf();

        // Load the hierarchical format
        let store = SettingsStore::load_from_path(path.clone()).unwrap();

        let bool_key = BoolSettingKey::new("appearance.autotheming", false);
        let string_key = StringSettingKey::new("workspace.layout", "tile");

        assert_eq!(store.bool(bool_key), true);
        assert_eq!(store.string(string_key), "monocle");

        // Save should maintain hierarchical format
        store.save().unwrap();

        let saved_content = fs::read_to_string(&path).unwrap();
        assert!(saved_content.contains("[appearance]"));
        assert!(saved_content.contains("[workspace]"));
        assert!(!saved_content.contains("[values]"));
        assert!(!saved_content.contains("\"appearance.autotheming\""));
    }

    #[test]
    fn test_empty_store() {
        let store = SettingsStore {
            path: PathBuf::from("/tmp/test"),
            data: SettingsFile::default(),
        };

        assert!(store.is_empty());

        let key = BoolSettingKey::new("test.key", true);
        assert_eq!(store.bool(key), true); // Should return default
    }
}
