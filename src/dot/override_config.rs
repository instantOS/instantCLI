//! Dotfile source override management
//!
//! Allows users to manually specify which repository/subdirectory a dotfile
//! should be sourced from, overriding the default priority-based resolution.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::common::paths;
use crate::common::TildePath;
use crate::dot::config::Config;
use crate::dot::dotfile::Dotfile;
use crate::dot::localrepo::LocalRepo;

/// A single dotfile source override
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DotfileOverride {
    /// Target path in home directory (e.g., "~/.config/kitty/kitty.conf")
    pub target_path: TildePath,
    /// Name of the source repository
    pub source_repo: String,
    /// Name of the subdirectory within the repo (e.g., "dots", "themes")
    pub source_subdir: String,
}

/// Configuration file for dotfile overrides
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct OverrideConfig {
    #[serde(default)]
    pub overrides: Vec<DotfileOverride>,
}

/// Represents an available source for a dotfile
#[derive(Debug, Clone)]
pub struct DotfileSource {
    pub repo_name: String,
    pub subdir_name: String,
    pub source_path: PathBuf,
}

impl std::fmt::Display for DotfileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} / {}", self.repo_name, self.subdir_name)
    }
}

/// Get the path to the overrides config file
pub fn overrides_file_path() -> Result<PathBuf> {
    Ok(paths::instant_config_dir()?.join("dot_overrides.toml"))
}

impl OverrideConfig {
    /// Load overrides from the config file
    pub fn load() -> Result<Self> {
        let path = overrides_file_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("reading overrides file {}", path.display()))?;
        toml::from_str(&content).context("parsing overrides config")
    }

    /// Save overrides to the config file
    pub fn save(&self) -> Result<()> {
        let path = overrides_file_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("creating config directory")?;
        }
        let content = toml::to_string_pretty(self).context("serializing overrides")?;
        fs::write(&path, content).context("writing overrides file")?;
        Ok(())
    }

    /// Add or update an override for a target path
    pub fn set_override(
        &mut self,
        target_path: PathBuf,
        source_repo: String,
        source_subdir: String,
    ) -> Result<()> {
        let target = TildePath::new(target_path);

        // Remove existing override for this path if any
        self.overrides
            .retain(|o| o.target_path.as_path() != target.as_path());

        // Add new override
        self.overrides.push(DotfileOverride {
            target_path: target,
            source_repo,
            source_subdir,
        });

        self.save()
    }

    /// Remove an override for a target path
    pub fn remove_override(&mut self, target_path: &Path) -> Result<bool> {
        let original_len = self.overrides.len();
        self.overrides
            .retain(|o| o.target_path.as_path() != target_path);

        if self.overrides.len() < original_len {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get override for a specific target path
    pub fn get_override(&self, target_path: &Path) -> Option<&DotfileOverride> {
        self.overrides
            .iter()
            .find(|o| o.target_path.as_path() == target_path)
    }

    /// Check if a path has an override
    pub fn has_override(&self, target_path: &Path) -> bool {
        self.get_override(target_path).is_some()
    }

    /// Build a lookup map for efficient override checking
    pub fn build_lookup_map(&self) -> HashMap<PathBuf, &DotfileOverride> {
        self.overrides
            .iter()
            .map(|o| (o.target_path.as_path().to_path_buf(), o))
            .collect()
    }
}

/// Find all available sources for a dotfile across all repos and subdirs
pub fn find_all_sources(config: &Config, target_path: &Path) -> Result<Vec<DotfileSource>> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
    let mut sources = Vec::new();

    for repo_config in &config.repos {
        if !repo_config.enabled {
            continue;
        }

        let local_repo = match LocalRepo::new(config, repo_config.name.clone()) {
            Ok(repo) => repo,
            Err(_) => continue,
        };

        for dotfile_dir in local_repo.active_dotfile_dirs() {
            let source_path = dotfile_dir.path.join(relative_path);
            if source_path.exists() {
                let subdir_name = dotfile_dir
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                sources.push(DotfileSource {
                    repo_name: repo_config.name.clone(),
                    subdir_name,
                    source_path,
                });
            }
        }
    }

    Ok(sources)
}

/// Apply overrides to a merged dotfiles map
///
/// This modifies the source_path of dotfiles that have an active override,
/// pointing them to the override source instead of the default. Overrides
/// are only applied when the source repo/subdir is enabled and active.
pub fn apply_overrides(
    dotfiles: &mut HashMap<PathBuf, Dotfile>,
    overrides: &OverrideConfig,
    config: &Config,
) -> Result<()> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let lookup = overrides.build_lookup_map();
    let mut active_subdirs_by_repo: HashMap<String, HashSet<String>> = HashMap::new();

    for repo in &config.repos {
        if !repo.enabled {
            continue;
        }

        active_subdirs_by_repo.insert(
            repo.name.clone(),
            config.resolve_active_subdirs(repo).into_iter().collect(),
        );
    }

    for (target_path, dotfile) in dotfiles.iter_mut() {
        if let Some(override_entry) = lookup.get(target_path) {
            let Some(active_subdirs) = active_subdirs_by_repo.get(&override_entry.source_repo)
            else {
                continue;
            };

            if !active_subdirs.contains(&override_entry.source_subdir) {
                continue;
            }

            // Construct the overridden source path
            let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
            let repo_path = config.repos_path().join(&override_entry.source_repo);
            let source_path = repo_path
                .join(&override_entry.source_subdir)
                .join(relative_path);

            // Only apply if the override source actually exists
            if source_path.exists() {
                dotfile.source_path = source_path;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_override_config_roundtrip() {
        let temp = tempdir().unwrap();
        let config_path = temp.path().join("overrides.toml");

        let mut config = OverrideConfig::default();
        config.overrides.push(DotfileOverride {
            target_path: TildePath::new(PathBuf::from("/home/test/.bashrc")),
            source_repo: "my-dots".to_string(),
            source_subdir: "themes".to_string(),
        });

        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&config_path, &content).unwrap();

        let loaded_content = fs::read_to_string(&config_path).unwrap();
        let loaded: OverrideConfig = toml::from_str(&loaded_content).unwrap();

        assert_eq!(loaded.overrides.len(), 1);
        assert_eq!(loaded.overrides[0].source_repo, "my-dots");
    }
}
