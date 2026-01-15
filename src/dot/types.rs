use crate::dot::config;
use crate::dot::localrepo::DotfileDir;
use crate::menu_utils::FzfSelectable;
use serde::{Deserialize, Serialize};

/// Repository metadata structure.
/// This is used for reading from instantdots.toml OR from the main config.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct RepoMetaData {
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

/// Helper struct for repository selection
#[derive(Debug, Clone)]
pub struct RepoSelectItem {
    pub repo: config::Repo,
}

impl FzfSelectable for RepoSelectItem {
    fn fzf_display_text(&self) -> String {
        self.repo.name.clone()
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(format!(
            "URL: {}\nBranch: {}\nEnabled: {}",
            self.repo.url,
            self.repo.branch.as_deref().unwrap_or("default"),
            if self.repo.enabled { "Yes" } else { "No" }
        ))
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
        self.dots_dir
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.dots_dir.path.display().to_string())
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(format!(
            "Repository: {}\nPath: {}\nActive: {}",
            self.repo_name,
            self.dots_dir.path.display(),
            if self.dots_dir.is_active { "Yes" } else { "No" }
        ))
    }
}
