use crate::dev::github::GitHubRepo;
use crate::dev::package::Package;
use fzf_wrapped::{Border, Fzf, Layout, run_with_output};

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

pub fn select_repository(repos: Vec<GitHubRepo>) -> Result<GitHubRepo, FzfError> {
    if repos.is_empty() {
        return Err(FzfError::NoRepositories);
    }

    let mut items = Vec::new();
    for repo in &repos {
        let display_line = format!(
            "{} - {}",
            repo.name,
            repo.description.as_deref().unwrap_or("No description")
        );
        items.push(display_line);
    }

    let fzf = Fzf::builder()
        .layout(Layout::Reverse)
        .border(Border::Rounded)
        .header("Select instantOS repository to clone:")
        .custom_args(vec!["--height=40%".to_string()])
        .build()
        .map_err(|e| FzfError::FzfError(format!("Failed to build Fzf: {}", e)))?;

    match run_with_output(fzf, items) {
        Some(selected) => {
            for repo in repos {
                if selected.starts_with(&repo.name) {
                    return Ok(repo);
                }
            }
            Err(FzfError::FzfError("Invalid selection format".to_string()))
        }
        None => Err(FzfError::UserCancelled),
    }
}

pub fn select_package(packages: Vec<Package>) -> Result<Package, FzfError> {
    if packages.is_empty() {
        return Err(FzfError::NoPackages);
    }

    let mut items = Vec::new();
    for package in &packages {
        let display_line = if let Some(desc) = &package.description {
            format!("{} - {}", package.name, desc)
        } else {
            package.name.clone()
        };
        items.push(display_line);
    }

    let fzf = Fzf::builder()
        .layout(Layout::Reverse)
        .border(Border::Rounded)
        .header("Select instantOS package to install:")
        .custom_args(vec!["--height=40%".to_string()])
        .build()
        .map_err(|e| FzfError::FzfError(format!("Failed to build Fzf: {}", e)))?;

    match run_with_output(fzf, items) {
        Some(selected) => {
            for package in packages {
                if selected.starts_with(&package.name) {
                    return Ok(package);
                }
            }
            Err(FzfError::FzfError("Invalid selection format".to_string()))
        }
        None => Err(FzfError::UserCancelled),
    }
}
