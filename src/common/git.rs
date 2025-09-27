use anyhow::{Context, Result};
use git2::{
    FetchOptions, Repository,
    build::{CheckoutBuilder, RepoBuilder},
};
use std::path::Path;

/// Clone a repository with optional branch and depth
pub fn clone_repo(
    url: &str,
    target: &Path,
    branch: Option<&str>,
    depth: Option<i32>,
) -> Result<Repository> {
    let mut fetch_options = FetchOptions::new();

    // Configure shallow clone if depth is specified
    if let Some(depth) = depth {
        fetch_options.depth(depth);
    }

    fetch_options.remote_callbacks(git2::RemoteCallbacks::new());

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch_options);

    if let Some(branch_name) = branch {
        builder.branch(branch_name);
    }

    let repo = builder
        .clone(url, target)
        .context("Failed to clone repository")?;

    Ok(repo)
}

/// Get the current checked out branch name
pub fn current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Failed to get HEAD reference")?;

    let head_name = head
        .shorthand()
        .ok_or_else(|| anyhow::anyhow!("HEAD is detached"))?;

    Ok(head_name.to_string())
}

/// Fetch a specific branch from origin
pub fn fetch_branch(repo: &mut Repository, branch: &str) -> Result<()> {
    let mut remote = repo
        .find_remote("origin")
        .context("Failed to find origin remote")?;

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(git2::RemoteCallbacks::new());

    remote
        .fetch(&[branch], Some(&mut fetch_options), None)
        .context("Failed to fetch branch")?;

    Ok(())
}

/// Checkout a specific branch
pub fn checkout_branch(repo: &mut Repository, branch: &str) -> Result<()> {
    // First, try to find the remote branch
    let remote_branch_name = format!("origin/{branch}");
    let remote_branch = repo.find_branch(&remote_branch_name, git2::BranchType::Remote);

    let _commit_id = match remote_branch {
        Ok(branch) => branch
            .get()
            .target()
            .ok_or_else(|| anyhow::anyhow!("Remote branch has no target"))?,
        Err(_) => {
            // If remote branch not found, try to find local branch
            let local_branch = repo
                .find_branch(branch, git2::BranchType::Local)
                .context("Branch not found locally or remotely")?;
            local_branch
                .get()
                .target()
                .ok_or_else(|| anyhow::anyhow!("Local branch has no target"))?
        }
    };

    // Checkout the commit
    repo.set_head(&format!("refs/heads/{branch}"))
        .context("Failed to set HEAD")?;

    repo.checkout_head(Some(
        &mut CheckoutBuilder::new()
            .force()
            .remove_ignored(true)
            .remove_untracked(true),
    ))?;

    Ok(())
}

/// Clean working directory and pull latest changes (fetch + reset)
pub fn clean_and_pull(repo: &mut Repository) -> Result<()> {
    // Get current branch
    let branch_name = current_branch(repo)?;

    // Check if working directory is dirty
    if repo.statuses(None)?.is_empty() {
        // Working directory is clean, just fetch
        fetch_branch(repo, &branch_name)?;
    } else {
        // Working directory is dirty, clean it
        repo.reset_default(None, None::<&str>)?;

        // Discard all changes
        repo.checkout_head(Some(
            &mut CheckoutBuilder::new()
                .force()
                .remove_ignored(true)
                .remove_untracked(true),
        ))?;

        // Now fetch
        fetch_branch(repo, &branch_name)?;
    }

    // Get the reference for the remote branch
    let remote_branch_name = format!("origin/{branch_name}");
    let remote_branch_ref = repo
        .find_reference(&remote_branch_name)
        .context("Failed to find remote branch reference")?;

    let remote_commit = remote_branch_ref
        .peel_to_commit()
        .context("Failed to peel remote branch to commit")?;

    // Reset local branch to match remote (hard reset)
    repo.set_head(&format!("refs/heads/{branch_name}"))
        .context("Failed to set HEAD")?;

    repo.reset(&remote_commit.into_object(), git2::ResetType::Hard, None)
        .context("Failed to reset to remote commit")?;

    Ok(())
}
