use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::common::config::DocumentedConfig;
use crate::common::paths;
use crate::documented_config;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ResolvethingConfig {
    pub working_directory: PathBuf,
    pub conflict_file_types: Vec<String>,
    pub editor_command: Option<String>,
}

impl Default for ResolvethingConfig {
    fn default() -> Self {
        Self {
            working_directory: default_working_directory(),
            conflict_file_types: vec!["md".to_string(), "json".to_string()],
            editor_command: None,
        }
    }
}

documented_config!(
    ResolvethingConfig,
    working_directory, "Directory scanned for duplicates and Syncthing conflict files",
    conflict_file_types, "File extensions treated as mergeable Syncthing conflicts",
    editor_command, "Optional editor command used for conflict diffs; defaults to $EDITOR or nvim",
    => paths::instant_config_dir()?.join("resolvething.toml")
);

impl ResolvethingConfig {
    pub fn load() -> Result<Self> {
        <Self as DocumentedConfig>::load_from_path_documented(Self::config_path()?)
            .context("loading resolvething config")
    }

    pub fn save(&self) -> Result<()> {
        self.save_with_documentation(&Self::config_path()?)
            .context("saving resolvething config")
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(paths::instant_config_dir()?.join("resolvething.toml"))
    }

    pub fn resolve_working_directory(&self, override_path: Option<&str>) -> Result<PathBuf> {
        let raw = override_path
            .map(str::to_string)
            .unwrap_or_else(|| self.working_directory.to_string_lossy().to_string());
        expand_path(&raw)
    }

    pub fn normalized_conflict_types(&self, override_types: &[String]) -> Vec<String> {
        let source = if override_types.is_empty() {
            &self.conflict_file_types
        } else {
            override_types
        };

        let mut out = Vec::new();
        for ty in source {
            let normalized = ty.trim().trim_start_matches('.').to_ascii_lowercase();
            if !normalized.is_empty() && !out.contains(&normalized) {
                out.push(normalized);
            }
        }
        out
    }
}

pub fn expand_path(raw: &str) -> Result<PathBuf> {
    let expanded = shellexpand::full(raw)
        .with_context(|| format!("expanding path '{}'", raw))?
        .into_owned();
    Ok(PathBuf::from(expanded))
}

pub fn format_path(path: &Path) -> String {
    crate::common::TildePath::new(path.to_path_buf()).display_string()
}

fn default_working_directory() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wiki")
        .join("vimwiki")
}
