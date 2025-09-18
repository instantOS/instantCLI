use crate::dev::github::GitHubRepo;
use crate::dev::package::Package;
use crate::fzf_wrapper::{FzfOptions, FzfPreview, FzfSelectable, FzfWrapper};
use anyhow::Result;

#[derive(thiserror::Error, Debug)]
pub enum FzfError {
    #[error("FZF error: {0}")]
    FzfError(String),

    #[error("User cancelled selection")]
    UserCancelled,

    #[error("No repositories available")]
    NoRepositories,

    #[error("No packages available")]
    NoPackages,
}

/// Helper struct for GitHub repository selection
#[derive(Debug, Clone)]
pub struct GitHubRepoSelectItem {
    pub repo: GitHubRepo,
}

impl FzfSelectable for GitHubRepoSelectItem {
    fn fzf_display_text(&self) -> String {
        format!(
            "{} - {}",
            self.repo.name,
            self.repo.description.as_deref().unwrap_or("No description")
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(format!(
            "Name: {}\nDescription: {}\nLanguage: {}\nStars: {}\nForks: {}",
            self.repo.name,
            self.repo.description.as_deref().unwrap_or("No description"),
            self.repo.language.as_deref().unwrap_or("Not specified"),
            self.repo.stargazers_count.unwrap_or(0),
            self.repo.forks_count.unwrap_or(0)
        ))
    }
}

/// Helper struct for package selection
#[derive(Debug, Clone)]
pub struct PackageSelectItem {
    pub package: Package,
}

impl FzfSelectable for PackageSelectItem {
    fn fzf_display_text(&self) -> String {
        if let Some(desc) = &self.package.description {
            format!("{} - {}", self.package.name, desc)
        } else {
            self.package.name.clone()
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(format!(
            "Name: {}\nDescription: {}\nPath: {}",
            self.package.name,
            self.package
                .description
                .as_deref()
                .unwrap_or("No description"),
            self.package.path.display()
        ))
    }
}

pub fn select_repository(repos: Vec<GitHubRepo>) -> Result<GitHubRepo, FzfError> {
    if repos.is_empty() {
        return Err(FzfError::NoRepositories);
    }

    let items: Vec<GitHubRepoSelectItem> = repos
        .into_iter()
        .map(|repo| GitHubRepoSelectItem { repo })
        .collect();

    let wrapper = FzfWrapper::with_options(FzfOptions {
        prompt: Some("Select instantOS repository to clone: ".to_string()),
        additional_args: vec!["--reverse".to_string()],
        ..Default::default()
    });

    match wrapper
        .select(items)
        .map_err(|e| FzfError::FzfError(format!("Selection error: {e}")))?
    {
        crate::fzf_wrapper::FzfResult::Selected(item) => Ok(item.repo),
        crate::fzf_wrapper::FzfResult::Cancelled => Err(FzfError::UserCancelled),
        crate::fzf_wrapper::FzfResult::Error(e) => Err(FzfError::FzfError(e)),
        _ => Err(FzfError::FzfError(
            "Unexpected selection result".to_string(),
        )),
    }
}

pub fn select_package(packages: Vec<Package>) -> Result<Package, FzfError> {
    if packages.is_empty() {
        return Err(FzfError::NoPackages);
    }

    let items: Vec<PackageSelectItem> = packages
        .into_iter()
        .map(|package| PackageSelectItem { package })
        .collect();

    let wrapper = FzfWrapper::with_options(FzfOptions {
        prompt: Some("Select instantOS package to install: ".to_string()),
        additional_args: vec!["--reverse".to_string()],
        ..Default::default()
    });

    match wrapper
        .select(items)
        .map_err(|e| FzfError::FzfError(format!("Selection error: {e}")))?
    {
        crate::fzf_wrapper::FzfResult::Selected(item) => Ok(item.package),
        crate::fzf_wrapper::FzfResult::Cancelled => Err(FzfError::UserCancelled),
        crate::fzf_wrapper::FzfResult::Error(e) => Err(FzfError::FzfError(e)),
        _ => Err(FzfError::FzfError(
            "Unexpected selection result".to_string(),
        )),
    }
}
