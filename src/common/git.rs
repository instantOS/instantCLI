use anyhow::{Context, Result};
use git2::{
    FetchOptions, Repository, Status, StatusOptions,
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

/// Fetch a specific branch from its configured remote (or origin)
pub fn fetch_branch(repo: &mut Repository, branch: &str) -> Result<()> {
    // Determine which remote to fetch from
    let remote_name = if let Ok(local_branch) = repo.find_branch(branch, git2::BranchType::Local) {
        // Branch exists locally, check upstream
        if let Ok(_upstream) = local_branch.upstream() {
            // Upstream exists, get its remote
            let buf = repo.branch_upstream_remote(&format!("refs/heads/{}", branch));
            match buf {
                Ok(buf) => buf.as_str().unwrap_or("origin").to_string(),
                Err(_) => "origin".to_string(), // Fallback
            }
        } else {
            // Branch exists but has no upstream.
            // Check if 'origin' exists as a fallback
            if repo.find_remote("origin").is_ok() {
                "origin".to_string()
            } else {
                return Ok(()); // No upstream, no origin. Skip.
            }
        }
    } else {
        // Branch does not exist locally. Assume we want to fetch it from origin.
        if repo.find_remote("origin").is_ok() {
            "origin".to_string()
        } else {
            return Ok(()); // No origin. Skip.
        }
    };

    let mut remote = repo
        .find_remote(&remote_name)
        .context(format!("Failed to find remote '{}'", remote_name))?;

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

    // Check if upstream exists - if not, we can't pull anything
    // Use a block to ensure local_branch is dropped before we borrow repo mutably
    let has_upstream = {
        let local_branch = repo.find_branch(&branch_name, git2::BranchType::Local)?;
        local_branch.upstream().is_ok()
    };

    if !has_upstream {
        return Ok(());
    }

    // Check if working directory is dirty
    if repo.statuses(None)?.is_empty() {
        // Working directory is clean, just fetch
        fetch_branch(repo, &branch_name)?;
    } else {
        // Working directory is dirty, clean it by discarding all changes
        // Force checkout HEAD and remove ignored/untracked files
        repo.checkout_head(Some(
            &mut CheckoutBuilder::new()
                .force()
                .remove_ignored(true)
                .remove_untracked(true),
        ))?;

        // Now fetch
        fetch_branch(repo, &branch_name)?;
    }

    // Get the upstream branch to reset to
    // We re-query the branch/upstream because fetch might have updated refs
    let local_branch = repo.find_branch(&branch_name, git2::BranchType::Local)?;
    let upstream = local_branch
        .upstream()
        .context("Upstream branch not found")?;
    let upstream_commit = upstream
        .get()
        .peel_to_commit()
        .context("Failed to peel upstream branch to commit")?;

    // Reset local branch to match upstream (hard reset)
    repo.set_head(&format!("refs/heads/{branch_name}"))
        .context("Failed to set HEAD")?;

    repo.reset(&upstream_commit.into_object(), git2::ResetType::Hard, None)
        .context("Failed to reset to upstream commit")?;

    Ok(())
}

/// Fetch and fast-forward: a non-destructive update that preserves local changes.
/// If the working directory has modifications or the branches have diverged,
/// the update is skipped with a warning instead of discarding local work.
pub fn fetch_and_fast_forward(repo: &mut Repository) -> Result<()> {
    let branch_name = current_branch(repo)?;

    // Check if upstream exists
    let has_upstream = {
        let local_branch = repo.find_branch(&branch_name, git2::BranchType::Local)?;
        local_branch.upstream().is_ok()
    };

    if !has_upstream {
        return Ok(());
    }

    // Fetch latest
    fetch_branch(repo, &branch_name)?;

    // Re-query after fetch
    let local_branch = repo.find_branch(&branch_name, git2::BranchType::Local)?;
    let upstream = local_branch
        .upstream()
        .context("Upstream branch not found")?;

    let local_commit = local_branch.get().peel_to_commit()?;
    let upstream_commit = upstream.get().peel_to_commit()?;

    if local_commit.id() == upstream_commit.id() {
        // Already up to date
        return Ok(());
    }

    let (ahead, behind) = repo.graph_ahead_behind(local_commit.id(), upstream_commit.id())?;

    if ahead > 0 && behind > 0 {
        anyhow::bail!(
            "Local branch '{}' has diverged from upstream ({} ahead, {} behind). \
             Resolve manually or mark the repository as read-only to force-update.",
            branch_name,
            ahead,
            behind,
        );
    }

    if ahead > 0 {
        // Local is ahead of upstream — nothing to pull
        return Ok(());
    }

    // Behind only — safe to fast-forward, but check for dirty working tree first
    let is_dirty = !repo.statuses(None)?.is_empty();
    if is_dirty {
        anyhow::bail!(
            "Working directory has local changes. \
             Commit or stash them before updating, \
             or mark the repository as read-only to force-update."
        );
    }

    // Fast-forward: move the branch ref to the upstream commit
    repo.set_head(&format!("refs/heads/{branch_name}"))
        .context("Failed to set HEAD")?;

    repo.reset(
        &upstream_commit.into_object(),
        git2::ResetType::Hard,
        None,
    )
    .context("Failed to fast-forward to upstream commit")?;

    Ok(())
}

/// Detailed repository status information
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub branch: String,
    pub working_dir_clean: bool,
    pub file_counts: FileStatusCounts,
    pub branch_sync: BranchSyncStatus,
}

/// File count by status
#[derive(Debug, Clone, Default)]
pub struct FileStatusCounts {
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    pub untracked: usize,
}

/// Branch synchronization status
#[derive(Debug, Clone)]
pub enum BranchSyncStatus {
    UpToDate,
    Ahead { commits: usize },
    Behind { commits: usize },
    Diverged { ahead: usize, behind: usize },
    NoRemote,
}

/// Get comprehensive git repository status
pub fn get_repo_status(repo: &Repository) -> Result<RepoStatus> {
    let branch = current_branch(repo)?;
    let statuses = repo.statuses(Some(
        StatusOptions::new()
            .include_untracked(true)
            .recurse_untracked_dirs(true),
    ))?;

    let file_counts = count_file_statuses(&statuses);
    let working_dir_clean = file_counts.modified == 0
        && file_counts.added == 0
        && file_counts.deleted == 0
        && file_counts.untracked == 0;

    let branch_sync = compare_with_remote(repo, &branch)?;

    Ok(RepoStatus {
        branch,
        working_dir_clean,
        file_counts,
        branch_sync,
    })
}

/// Count files by their git status
pub fn count_file_statuses(statuses: &git2::Statuses) -> FileStatusCounts {
    let mut counts = FileStatusCounts::default();

    for entry in statuses.iter() {
        let status = entry.status();

        if status.contains(Status::WT_NEW) {
            counts.untracked += 1;
        }
        if status.contains(Status::WT_MODIFIED) {
            counts.modified += 1;
        }
        if status.contains(Status::WT_DELETED) {
            counts.deleted += 1;
        }
        if status.contains(Status::INDEX_NEW) {
            counts.added += 1;
        }
        if status.contains(Status::INDEX_MODIFIED) || status.contains(Status::INDEX_DELETED) {
            counts.modified += 1;
        }
    }

    counts
}

/// Compare local branch with remote tracking branch
pub fn compare_with_remote(repo: &Repository, branch_name: &str) -> Result<BranchSyncStatus> {
    let local_branch = repo.find_branch(branch_name, git2::BranchType::Local);

    let local_branch = match local_branch {
        Ok(branch) => branch,
        Err(_) => return Ok(BranchSyncStatus::NoRemote),
    };

    let upstream = local_branch.upstream();

    let upstream = match upstream {
        Ok(branch) => branch,
        Err(_) => return Ok(BranchSyncStatus::NoRemote),
    };

    let local_commit = local_branch.get().peel_to_commit()?;
    let upstream_commit = upstream.get().peel_to_commit()?;

    let (ahead, behind) = repo.graph_ahead_behind(local_commit.id(), upstream_commit.id())?;

    match (ahead, behind) {
        (0, 0) => Ok(BranchSyncStatus::UpToDate),
        (a, 0) => Ok(BranchSyncStatus::Ahead { commits: a }),
        (0, b) => Ok(BranchSyncStatus::Behind { commits: b }),
        (a, b) => Ok(BranchSyncStatus::Diverged {
            ahead: a,
            behind: b,
        }),
    }
}
