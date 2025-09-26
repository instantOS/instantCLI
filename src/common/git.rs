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
    let remote_branch_name = format!("origin/{}", branch);
    let remote_branch = repo.find_branch(&remote_branch_name, git2::BranchType::Remote);

    let commit_id = match remote_branch {
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

    // TODO: is this needed? Maybe remove
    let _commit = repo
        .find_commit(commit_id)
        .context("Failed to find commit")?;

    // Checkout the commit
    repo.set_head(&format!("refs/heads/{}", branch))
        .context("Failed to set HEAD")?;

    repo.checkout_head(Some(
        &mut CheckoutBuilder::new()
            .force()
            .remove_ignored(true)
            .remove_untracked(true),
    ))?;

    Ok(())
}

/// Pull latest changes (fetch + merge)
// TODO: this should be simplified as well as renamed. If the working directory is dirty, clean it
// and then pull. No merging
pub fn pull(repo: &mut Repository) -> Result<()> {
    // Get current branch
    let branch_name = current_branch(repo)?;

    // Fetch from origin
    fetch_branch(repo, &branch_name)?;

    // Get the reference for the remote branch
    let remote_branch_name = format!("origin/{}", branch_name);
    let remote_branch_ref = repo
        .find_reference(&remote_branch_name)
        .context("Failed to find remote branch reference")?;

    let remote_commit = remote_branch_ref
        .peel_to_commit()
        .context("Failed to peel remote branch to commit")?;

    // Get the reference for the local branch
    let local_branch_ref = repo
        .find_branch(&branch_name, git2::BranchType::Local)
        .context("Failed to find local branch")?;

    let local_branch_ref = local_branch_ref.into_reference();
    let annotated_commit = repo.reference_to_annotated_commit(&local_branch_ref)?;

    // Perform the merge
    let mut merge_options = git2::MergeOptions::new();
    merge_options.fail_on_conflict(true);

    repo.merge(&[&annotated_commit], Some(&mut merge_options), None)
        .context("Failed to perform merge")?;

    // Check if there are conflicts
    if repo.index().unwrap().has_conflicts() {
        return Err(anyhow::anyhow!("Merge conflicts detected"));
    }

    // Create commit for the merge
    let signature = repo.signature().context("Failed to get git signature")?;

    let mut index = repo.index()?;
    let tree_id = index.write_tree().context("Failed to write tree")?;

    let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

    let parent_commit = repo.head()?.peel_to_commit()?;

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        &format!("Merge branch '{}' of origin", branch_name),
        &tree,
        &[&parent_commit, &remote_commit],
    )
    .context("Failed to create merge commit")?;

    // Clean up the merge state
    repo.cleanup_state()?;

    Ok(())
}

/// Check if the path is a git repository
// TODO: this function is redundant, it could be replaced with using git2 directly
pub fn is_git_repo(path: &Path) -> bool {
    Repository::open(path).is_ok()
}

/// Open an existing git repository
// TODO: this function is redundant, it could be replaced with using git2 directly
pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::open(path).context("Failed to open git repository")
}
