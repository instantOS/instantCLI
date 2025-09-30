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

    #[error("Target directory already exists: {0}")]
    DirectoryExists(String),
}

pub fn clone_repository(repo: &GitHubRepo, target_dir: &Path, _debug: bool) -> Result<()> {
    if target_dir.exists() {
        return Err(CloneError::DirectoryExists(target_dir.display().to_string()).into());
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

    pb.finish_with_message(format!("Successfully cloned {}", repo.name));

    result.map_err(|e| CloneError::GitError(e.to_string()))?;

    emit(
        Level::Success,
        "dev.clone.success",
        &format!(
            "{} Successfully cloned {} to {}",
            char::from(Fa::CheckCircle),
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
            char::from(Fa::InfoCircle),
            repo.html_url
        ),
        None,
    );

    if let Some(desc) = &repo.description {
        emit(
            Level::Info,
            "dev.clone.description",
            &format!(
                "{} {desc}",
                char::from(Fa::InfoCircle)
            ),
            None,
        );
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
                char::from(Fa::Folder),
                workspace_dir.display()
            ),
            None,
        );
    }

    Ok(workspace_dir)
}
