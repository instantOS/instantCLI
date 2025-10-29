use std::{fs, path::PathBuf};

use anyhow::{Context, Result};

use super::models::{UserSpec, UsersFile};

/// Persistent storage for user configurations
pub(super) struct UserStore {
    path: PathBuf,
    data: UsersFile,
}

impl UserStore {
    /// Load the user store from disk, creating it if it doesn't exist
    pub fn load() -> Result<Self> {
        let path = users_file_path()?;
        if !path.exists() {
            return Ok(Self {
                path,
                data: UsersFile::default(),
            });
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("reading user settings from {}", path.display()))?;
        let data = toml::from_str(&contents)
            .with_context(|| format!("parsing user settings at {}", path.display()))?;

        Ok(Self { path, data })
    }

    /// Save the user store to disk
    pub fn save(&self) -> Result<()> {
        let contents =
            toml::to_string_pretty(&self.data).context("serializing user settings to toml")?;
        fs::write(&self.path, contents)
            .with_context(|| format!("writing user settings to {}", self.path.display()))
    }

    /// Iterate over all users in the store
    pub fn iter(&self) -> impl Iterator<Item = (&String, &UserSpec)> {
        self.data.users.iter()
    }

    /// Get a user spec by username
    pub fn get(&self, username: &str) -> Option<&UserSpec> {
        self.data.users.get(username)
    }

    /// Insert or update a user spec
    pub fn insert(&mut self, username: &str, spec: UserSpec) {
        self.data.users.insert(username.to_string(), spec);
    }

    /// Remove a user from the store
    pub fn remove(&mut self, username: &str) {
        self.data.users.remove(username);
    }
}

/// Get the path to the users.toml file
fn users_file_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("unable to determine user config directory")?
        .join("instant");
    fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating config directory {}", config_dir.display()))?;
    Ok(config_dir.join("users.toml"))
}

