use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::common::paths;

fn default_show_logo() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct WallpaperConfig {
    pub path: Option<String>,
    #[serde(default = "default_show_logo")]
    pub show_logo: bool,
}

impl WallpaperConfig {
    pub fn config_file_path() -> Result<PathBuf> {
        Ok(paths::instant_config_dir()?.join("wallpaper.toml"))
    }

    pub fn load() -> Result<Self> {
        let cfg_path = Self::config_file_path()?;
        if !cfg_path.exists() {
            return Ok(Self::default());
        }

        let s = fs::read_to_string(&cfg_path)
            .with_context(|| format!("reading config {}", cfg_path.display()))?;
        toml::from_str(&s).context("parsing config toml")
    }

    pub fn save(&self) -> Result<()> {
        let cfg_path = Self::config_file_path()?;
        if let Some(parent) = cfg_path.parent() {
            fs::create_dir_all(parent).context("creating config directory")?;
        }

        let toml = toml::to_string_pretty(self).context("serializing config to toml")?;
        fs::write(&cfg_path, toml).context("writing config file")?;
        Ok(())
    }

    pub fn set_wallpaper(&mut self, path: String) -> Result<()> {
        // Resolve absolute path if possible
        let path_buf = PathBuf::from(&path);
        let abs_path = if path_buf.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .context("getting current directory")?
                .join(path)
                .to_string_lossy()
                .to_string()
        };

        self.path = Some(abs_path);
        self.save()
    }

    pub fn set_show_logo(&mut self, show_logo: bool) -> Result<()> {
        self.show_logo = show_logo;
        self.save()
    }
}
