use crate::dot::config;
use crate::dot::dotfilerepo::DotfileDir;
use crate::menu_utils::FzfSelectable;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Repository metadata structure.
/// This is used for reading from instantdots.toml OR from the main config.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct RepoMetaData {
    #[serde(default = "crate::dot::types::RepoMetaData::default_name")]
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub read_only: Option<bool>,
    #[serde(default = "default_dots_dirs")]
    pub dots_dirs: Vec<String>,
    #[serde(default)]
    pub default_active_subdirs: Option<Vec<String>>,
    /// Directories that should be treated as atomic units.
    /// If any file in a unit is modified, all files in that unit are treated as modified.
    #[serde(default)]
    pub units: Vec<String>,
}

impl RepoMetaData {
    fn default_name() -> String {
        "dotfiles".to_string()
    }
}

impl Default for RepoMetaData {
    fn default() -> Self {
        Self {
            name: Self::default_name(),
            author: None,
            description: None,
            read_only: None,
            dots_dirs: default_dots_dirs(),
            default_active_subdirs: None,
            units: Vec::new(),
        }
    }
}

fn default_dots_dirs() -> Vec<String> {
    vec!["dots".to_string()]
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoName(String);

impl RepoName {
    pub fn new(name: String) -> Self {
        RepoName(name)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RepoName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for RepoName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Repository selection item with precomputed preview for fzf menus
#[derive(Clone)]
pub struct RepoMenuItem {
    pub repo: config::Repo,
    pub preview: String,
}

impl FzfSelectable for RepoMenuItem {
    fn fzf_display_text(&self) -> String {
        use crate::ui::catppuccin::{colors, format_icon_colored};
        use crate::ui::nerd_font::NerdFont;

        let badge = if self.repo.metadata.is_some() {
            " \x1b[33m[external]\x1b[0m".to_string()
        } else {
            String::new()
        };
        format!(
            "{} {}{}",
            format_icon_colored(NerdFont::Folder, colors::MAUVE),
            self.repo.name,
            badge
        )
    }

    fn fzf_key(&self) -> String {
        self.repo.name.clone()
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(self.preview.clone())
    }
}

/// Helper struct for dots directory selection
#[derive(Debug, Clone)]
pub struct DotsDirSelectItem {
    pub dots_dir: DotfileDir,
    pub repo_name: String,
}

impl FzfSelectable for DotsDirSelectItem {
    fn fzf_display_text(&self) -> String {
        use crate::ui::catppuccin::{colors, format_icon_colored};
        use crate::ui::nerd_font::NerdFont;

        let name = self
            .dots_dir
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.dots_dir.path.display().to_string());

        let status_icon = if self.dots_dir.is_active {
            format_icon_colored(NerdFont::Check, colors::GREEN)
        } else {
            format_icon_colored(NerdFont::Folder, colors::SURFACE2)
        };

        format!("{} {}", status_icon, name)
    }

    fn fzf_key(&self) -> String {
        // Use a unique key combining repo name and subdir path
        let subdir_name = self
            .dots_dir
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.dots_dir.path.display().to_string());
        format!("{}/{}", self.repo_name, subdir_name)
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        use crate::ui::nerd_font::NerdFont;
        use crate::ui::preview::PreviewBuilder;

        PreviewBuilder::new()
            .header(NerdFont::Folder, "Dots Directory")
            .blank()
            .field("Repository", &self.repo_name)
            .field("Path", &self.dots_dir.path.display().to_string())
            .field("Active", if self.dots_dir.is_active { "Yes" } else { "No" })
            .build()
    }
}

// Import macro from crate root
use crate::documented_config;

// Implement DocumentedConfig trait for RepoMetaData using the macro
// Note: config_path returns a placeholder since instantdots.toml paths are dynamic per-repo
documented_config!(RepoMetaData,
    name, "Repository name (used for identification)",
    author, "Repository author/maintainer",
    description, "Repository description",
    read_only, "Whether repository is read-only (default: false)",
    dots_dirs, "Directories containing dotfiles (e.g., ['dots'])",
    default_active_subdirs, "Default active subdirectories (defaults to first in dots_dirs)",
    units, "Directories treated as atomic units (all files modified together)",
    => Ok(std::path::PathBuf::from("instantdots.toml"))
);

#[cfg(test)]
mod documented_config_tests {
    use super::*;
    use crate::common::config::DocumentedConfig;

    #[test]
    fn test_field_metadata_default_values() {
        let metadata = RepoMetaData::field_metadata();

        let subdirs_field = metadata
            .iter()
            .find(|f| f.name == "default_active_subdirs")
            .unwrap();
        assert_eq!(
            subdirs_field.default_value.as_deref(),
            Some("[]"),
            "default_active_subdirs should be []"
        );

        let units_field = metadata.iter().find(|f| f.name == "units").unwrap();
        assert_eq!(units_field.default_value.as_deref(), Some("[]"));
    }
}
