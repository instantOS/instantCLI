use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SettingsFile {
    #[serde(default)]
    values: BTreeMap<String, toml::Value>,
}

impl SettingsFile {
    pub fn values(&self) -> &BTreeMap<String, toml::Value> {
        &self.values
    }

    pub fn values_mut(&mut self) -> &mut BTreeMap<String, toml::Value> {
        &mut self.values
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
            .values()
            .get(key.key)
            .and_then(|value| value.as_bool())
            .unwrap_or(key.default)
    }

    pub fn set_bool(&mut self, key: BoolSettingKey, value: bool) {
        self.data
            .values_mut()
            .insert(key.key.to_string(), toml::Value::Boolean(value));
    }

    pub fn string(&self, key: StringSettingKey) -> String {
        self.data
            .values()
            .get(key.key)
            .and_then(|value| value.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| key.default.to_string())
    }

    pub fn set_string<S: Into<String>>(&mut self, key: StringSettingKey, value: S) {
        self.data
            .values_mut()
            .insert(key.key.to_string(), toml::Value::String(value.into()));
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_empty(&self) -> bool {
        self.data.values().is_empty()
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
