use crate::common::git;
use crate::dev::github::GitHubRepo;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum CloneError {
    #[error("Git operation failed: {0}")]
    GitError(String),

    #[error("File system error: {0}")]
    FilesystemError(String),
}

pub fn clone_or_pull_repository(repo: &GitHubRepo, target_dir: &Path, _debug: bool) -> Result<()> {
    if target_dir.exists() {
        return pull_existing(repo, target_dir);
    }

    let pb = crate::common::progress::create_spinner(format!(
        "Cloning {} into {}...",
        repo.name,
        target_dir.display()
    ));

    let result = git::clone_repo(
        &repo.clone_url,
        target_dir,
        Some(&repo.default_branch),
        Some(1),
    );

    crate::common::progress::finish_spinner_with_success(
        pb,
        format!("Successfully cloned {}", repo.name),
    );

    result.map_err(|e| CloneError::GitError(e.to_string()))?;

    emit(
        Level::Success,
        "dev.clone.success",
        &format!(
            "{} Successfully cloned {} to {}",
            char::from(NerdFont::Check),
            repo.name,
            target_dir.display()
        ),
        None,
    );
    emit(
        Level::Info,
        "dev.clone.repo",
        &format!(
            "{} Repository: {}",
            char::from(NerdFont::Info),
            repo.html_url
        ),
        None,
    );

    if let Some(desc) = &repo.description {
        emit(
            Level::Info,
            "dev.clone.description",
            &format!("{} {desc}", char::from(NerdFont::Info)),
            None,
        );
    }

    Ok(())
}

fn pull_existing(repo: &GitHubRepo, target_dir: &Path) -> Result<()> {
    let pb = crate::common::progress::create_spinner(format!(
        "Pulling latest changes for {}...",
        repo.name
    ));

    let mut git_repo = git2::Repository::open(target_dir)
        .context("Directory exists but is not a git repository")?;

    let branch = git::current_branch(&git_repo).unwrap_or_else(|_| repo.default_branch.clone());

    let result = git::fetch_branch(&mut git_repo, &branch);

    match result {
        Ok(()) => {
            crate::common::progress::finish_spinner_with_success(
                pb,
                format!("Updated {}", repo.name),
            );
            emit(
                Level::Success,
                "dev.clone.pull_success",
                &format!(
                    "{} Pulled latest changes for {} ({})",
                    char::from(NerdFont::Check),
                    repo.name,
                    target_dir.display()
                ),
                None,
            );
        }
        Err(e) => {
            crate::common::progress::finish_spinner_with_success(
                pb,
                format!("Fetch failed for {}", repo.name),
            );
            emit(
                Level::Warn,
                "dev.clone.pull_failed",
                &format!(
                    "{} Could not pull latest changes: {}",
                    char::from(NerdFont::Warning),
                    e
                ),
                None,
            );
            emit(
                Level::Info,
                "dev.clone.pull_path",
                &format!(
                    "{} Repository is at {}",
                    char::from(NerdFont::Info),
                    target_dir.display()
                ),
                None,
            );
        }
    }

    Ok(())
}

pub fn ensure_workspace_dir() -> Result<std::path::PathBuf> {
    let home_dir = dirs::home_dir().ok_or_else(|| {
        CloneError::FilesystemError("Could not determine home directory".to_string())
    })?;

    let workspace_dir = home_dir.join("workspace");

    if !workspace_dir.exists() {
        std::fs::create_dir_all(&workspace_dir).context("Failed to create workspace directory")?;
        emit(
            Level::Info,
            "dev.clone.workspace_created",
            &format!(
                "{} Created workspace directory: {}",
                char::from(NerdFont::Folder),
                workspace_dir.display()
            ),
            None,
        );
    }

    Ok(workspace_dir)
}
