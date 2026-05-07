use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::{
    path::Path,
    process::{Command, Stdio},
};

/// Detects whether we are attached to an interactive terminal that can answer
/// SSH or git credential prompts. We require both stdin and stderr to be TTYs
/// so the prompt is visible *and* the user can respond to it.
fn is_interactive_terminal() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Configure a git command for non-interactive network use: tell git/ssh to
/// fail fast instead of hanging on a passphrase, host-key, or credential
/// prompt.
fn apply_noninteractive_git_env(cmd: &mut Command) {
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    let ssh_cmd = match std::env::var("GIT_SSH_COMMAND") {
        Ok(existing) if !existing.trim().is_empty() => {
            format!("{existing} -o BatchMode=yes -o ConnectTimeout=15")
        }
        _ => "ssh -o BatchMode=yes -o ConnectTimeout=15".to_string(),
    };
    cmd.env("GIT_SSH_COMMAND", ssh_cmd);
}

/// Translate captured git/ssh error output into a friendlier diagnostic for
/// the non-interactive case.
fn summarize_git_network_failure(args: &[&str], stderr: &str) -> String {
    let trimmed = stderr.trim();
    let lower = trimmed.to_ascii_lowercase();

    let hint = if lower.contains("permission denied (publickey")
        || lower.contains("permission denied, please try again")
        || lower.contains("could not read from remote repository")
    {
        Some(
            "SSH authentication failed and no terminal is available to prompt. \
             Load your key into ssh-agent (ssh-add), use a key without a passphrase, \
             or rerun the command in an interactive terminal.",
        )
    } else if lower.contains("enter passphrase")
        || lower.contains("passphrase for key")
        || lower.contains("batchmode")
    {
        Some(
            "SSH key requires a passphrase but no terminal is available to prompt. \
             Run `ssh-add` to unlock the key in your ssh-agent, or rerun in an \
             interactive terminal.",
        )
    } else if lower.contains("host key verification failed")
        || lower.contains("no matching host key")
        || lower.contains("host key for")
    {
        Some(
            "SSH host key is not trusted yet. Connect to the host once \
             interactively to accept the host key (or pre-populate ~/.ssh/known_hosts).",
        )
    } else if lower.contains("could not resolve hostname")
        || lower.contains("name or service not known")
    {
        Some("SSH hostname could not be resolved.")
    } else if lower.contains("connection timed out") || lower.contains("operation timed out") {
        Some("SSH connection timed out.")
    } else if lower.contains("terminal prompts disabled")
        || lower.contains("could not read username")
        || lower.contains("could not read password")
    {
        Some(
            "git asked for credentials but no terminal is available. Configure \
             a credential helper or rerun in an interactive terminal.",
        )
    } else {
        None
    };

    let prefix = format!("git {} failed", args.join(" "));
    match (hint, trimmed.is_empty()) {
        (Some(h), false) => format!("{prefix}: {h}\n{trimmed}"),
        (Some(h), true) => format!("{prefix}: {h}"),
        (None, false) => format!("{prefix}: {trimmed}"),
        (None, true) => prefix,
    }
}

/// Run a git command that talks to the network (clone/fetch/pull/push).
///
/// On an interactive terminal, stdio is inherited so SSH passphrase prompts,
/// host-key prompts, and git's own progress are visible to the user.
///
/// When no TTY is available we set `GIT_TERMINAL_PROMPT=0` and force SSH into
/// `BatchMode` so the operation fails fast with a clear message instead of
/// hanging forever waiting for input that nobody can provide.
fn run_git_network_in(current_dir: Option<&Path>, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }

    if is_interactive_terminal() {
        let status = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("Failed to execute git")?;

        if !status.success() {
            anyhow::bail!("git {} failed (exit status {})", args.join(" "), status);
        }
        return Ok(());
    }

    apply_noninteractive_git_env(&mut cmd);
    let output = cmd
        .stdin(Stdio::null())
        .output()
        .context("Failed to execute git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", summarize_git_network_failure(args, &stderr));
    }
    Ok(())
}

/// Run a git command in the given repo directory, returning stdout on success.
fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .context("Failed to execute git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a git command, returning (success, stdout) without failing on non-zero exit.
fn run_git_status(repo_path: &Path, args: &[&str]) -> Result<(bool, String)> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .context("Failed to execute git")?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((output.status.success(), stdout))
}

fn git_config_value(repo_path: &Path, key: &str) -> Result<Option<String>> {
    let (ok, value) = run_git_status(repo_path, &["config", "--get", key])?;
    if !ok || value.is_empty() {
        return Ok(None);
    }
    Ok(Some(value))
}

fn has_git_identity(repo_path: &Path) -> Result<bool> {
    let (has_author, _) = run_git_status(repo_path, &["var", "GIT_AUTHOR_IDENT"])?;
    if !has_author {
        return Ok(false);
    }

    let (has_committer, _) = run_git_status(repo_path, &["var", "GIT_COMMITTER_IDENT"])?;
    Ok(has_committer)
}

fn branch_upstream_remote(repo_path: &Path, branch: &str) -> Result<Option<String>> {
    let (ok, remote) = run_git_status(
        repo_path,
        &["config", "--get", &format!("branch.{branch}.remote")],
    )?;
    if !ok || remote.is_empty() {
        return Ok(None);
    }
    Ok(Some(remote))
}

fn remote_exists(repo_path: &Path, remote: &str) -> Result<bool> {
    let (exists, _) = run_git_status(repo_path, &["remote", "get-url", remote])?;
    Ok(exists)
}

fn list_remotes(repo_path: &Path) -> Result<Vec<String>> {
    let (ok, remotes) = run_git_status(repo_path, &["remote"])?;
    if !ok {
        return Ok(Vec::new());
    }

    Ok(remotes
        .lines()
        .map(str::trim)
        .filter(|remote| !remote.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn remote_for_branch(repo_path: &Path, branch: &str) -> Result<Option<String>> {
    if let Some(remote) = branch_upstream_remote(repo_path, branch)? {
        if remote_exists(repo_path, &remote)? {
            return Ok(Some(remote));
        }
    }

    if remote_exists(repo_path, "origin")? {
        return Ok(Some("origin".to_string()));
    }

    Ok(None)
}

fn remote_branch_ref(repo_path: &Path, branch: &str) -> Result<Option<(String, String)>> {
    let Some(remote) = remote_for_branch(repo_path, branch)? else {
        return Ok(None);
    };

    if remote_branch_exists_on(repo_path, &remote, branch)? {
        return Ok(Some((remote.clone(), format!("{remote}/{branch}"))));
    }

    Ok(None)
}

/// Clone a repository with optional branch and depth
pub fn clone_repo(
    url: &str,
    target: &Path,
    branch: Option<&str>,
    depth: Option<i32>,
) -> Result<()> {
    let depth_str = depth.filter(|d| *d > 0).map(|d| d.to_string());
    let target_str = target.to_string_lossy().into_owned();

    let mut args: Vec<&str> = vec!["clone"];
    if let Some(d) = depth_str.as_deref() {
        args.push("--depth");
        args.push(d);
    }
    if let Some(b) = branch {
        args.push("-b");
        args.push(b);
    }
    args.push(url);
    args.push(&target_str);

    run_git_network_in(None, &args).context("Failed to clone repository")
}

/// Get the current checked out branch name
pub fn current_branch(repo_path: &Path) -> Result<String> {
    let branch = run_git(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .context("Failed to get current branch")?;
    if branch == "HEAD" {
        anyhow::bail!("HEAD is detached");
    }
    Ok(branch)
}

/// Fetch a specific branch from its configured remote (or origin)
pub fn fetch_branch(repo_path: &Path, branch: &str) -> Result<()> {
    let Some(remote) = remote_for_branch(repo_path, branch)? else {
        return Ok(());
    };

    run_git_network_in(Some(repo_path), &["fetch", &remote, branch])
        .context("Failed to fetch branch")?;
    Ok(())
}

fn local_branch_exists(repo_path: &Path, branch: &str) -> Result<bool> {
    let (exists, _) = run_git_status(
        repo_path,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )?;
    Ok(exists)
}

fn remote_branch_exists_on(repo_path: &Path, remote: &str, branch: &str) -> Result<bool> {
    let (exists, _) = run_git_status(
        repo_path,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote}/{branch}"),
        ],
    )?;
    Ok(exists)
}

/// Checkout a specific branch, creating a local tracking branch if only the remote exists.
pub fn checkout_branch(repo_path: &Path, branch: &str) -> Result<()> {
    if local_branch_exists(repo_path, branch)? {
        run_git(repo_path, &["checkout", "-f", branch]).context("Failed to checkout branch")?;
        return Ok(());
    }

    if let Some((_remote, remote_ref)) = remote_branch_ref(repo_path, branch)? {
        run_git(
            repo_path,
            &["checkout", "-f", "-B", branch, "--track", &remote_ref],
        )
        .context("Failed to checkout remote tracking branch")?;
        return Ok(());
    }

    anyhow::bail!("Branch '{branch}' not found locally or on its configured remote");
}

fn has_upstream(repo_path: &Path) -> Result<bool> {
    let (has_upstream, _) = run_git_status(repo_path, &["rev-parse", "--abbrev-ref", "@{u}"])?;
    Ok(has_upstream)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultPushReadiness {
    Ready,
    NoRemote,
    NoUpstream { branch: String },
}

pub fn default_push_readiness(repo_path: &Path) -> Result<DefaultPushReadiness> {
    if list_remotes(repo_path)?.is_empty() {
        return Ok(DefaultPushReadiness::NoRemote);
    }

    let push_default =
        git_config_value(repo_path, "push.default")?.unwrap_or_else(|| "simple".to_string());

    if matches!(push_default.as_str(), "simple" | "upstream") && !has_upstream(repo_path)? {
        return Ok(DefaultPushReadiness::NoUpstream {
            branch: current_branch(repo_path)?,
        });
    }

    Ok(DefaultPushReadiness::Ready)
}

fn ahead_behind(repo_path: &Path) -> Result<Option<(usize, usize)>> {
    let (ok, counts) = run_git_status(
        repo_path,
        &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
    )?;
    if !ok {
        return Ok(None);
    }

    let parts: Vec<&str> = counts.split_whitespace().collect();
    if parts.len() != 2 {
        return Ok(None);
    }

    let ahead = parts[0].parse().unwrap_or(0);
    let behind = parts[1].parse().unwrap_or(0);
    Ok(Some((ahead, behind)))
}

fn worktree_status(repo_path: &Path) -> Result<String> {
    let (_, status_output) = run_git_status(
        repo_path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    Ok(status_output)
}

pub fn has_local_changes(repo_path: &Path) -> Result<bool> {
    Ok(!worktree_status(repo_path)?.is_empty())
}

pub fn stash_local_changes(repo_path: &Path, message: &str) -> Result<bool> {
    let status_before = worktree_status(repo_path)?;
    if status_before.is_empty() {
        return Ok(false);
    }

    run_git(repo_path, &["stash", "push", "-u", "-m", message])
        .context("Failed to stash local changes")?;

    Ok(worktree_status(repo_path)?.is_empty())
}

fn ensure_worktree_clean(repo_path: &Path) -> Result<()> {
    if !worktree_status(repo_path)?.is_empty() {
        anyhow::bail!(
            "Working directory has local changes. \
             Commit or stash them before updating, \
             or mark the repository as read-only to force-update."
        );
    }

    Ok(())
}

/// Clean working directory and pull latest changes (fetch + reset)
pub fn clean_and_pull(repo_path: &Path) -> Result<()> {
    if !has_upstream(repo_path)? {
        return Ok(());
    }

    // Discard local changes
    run_git(repo_path, &["reset", "--hard", "HEAD"])
        .context("Failed to discard tracked local changes")?;
    run_git(repo_path, &["clean", "-fdx"]).context("Failed to remove untracked local files")?;

    // Fetch (network: may need SSH auth, so use the network-aware helper)
    run_git_network_in(Some(repo_path), &["fetch"]).context("Failed to fetch")?;

    // Hard reset to upstream
    run_git(repo_path, &["reset", "--hard", "@{u}"]).context("Failed to reset to upstream")?;

    Ok(())
}

/// Fetch and fast-forward: a non-destructive update that preserves local changes.
/// If the working directory has modifications or the branches have diverged,
/// the update is skipped with a warning instead of discarding local work.
pub fn fetch_and_fast_forward(repo_path: &Path) -> Result<()> {
    if !has_upstream(repo_path)? {
        return Ok(());
    }

    // Fetch latest (network: may need SSH auth, so use the network-aware helper)
    run_git_network_in(Some(repo_path), &["fetch"]).context("Failed to fetch")?;

    // Check ahead/behind
    let Some((ahead, behind)) = ahead_behind(repo_path)? else {
        return Ok(());
    };

    if ahead == 0 && behind == 0 {
        return Ok(());
    }

    if ahead > 0 && behind > 0 {
        let branch_name = current_branch(repo_path).unwrap_or_default();
        anyhow::bail!(
            "Local branch '{}' has diverged from upstream ({} ahead, {} behind). \
             Resolve manually or mark the repository as read-only to force-update.",
            branch_name,
            ahead,
            behind,
        );
    }

    if ahead > 0 {
        return Ok(());
    }

    // Behind only — check for dirty working tree
    ensure_worktree_clean(repo_path)?;

    // Fast-forward
    run_git(repo_path, &["merge", "--ff-only", "@{u}"])
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
pub fn get_repo_status(repo_path: &Path) -> Result<RepoStatus> {
    let branch = current_branch(repo_path)?;

    // Parse porcelain status for file counts
    let status_output = worktree_status(repo_path)?;
    let file_counts = parse_porcelain_status(&status_output);

    let working_dir_clean = status_output.is_empty();

    let branch_sync = get_branch_sync(repo_path)?;

    Ok(RepoStatus {
        branch,
        working_dir_clean,
        file_counts,
        branch_sync,
    })
}

/// Parse `git status --porcelain` output into file status counts
fn parse_porcelain_status(output: &str) -> FileStatusCounts {
    let mut counts = FileStatusCounts::default();

    for line in output.lines() {
        if line.len() < 2 {
            continue;
        }
        let index = line.as_bytes()[0];
        let worktree = line.as_bytes()[1];

        if index == b'?' && worktree == b'?' {
            counts.untracked += 1;
            continue;
        }

        for status in [index, worktree] {
            match status {
                b'A' => counts.added += 1,
                b'D' => counts.deleted += 1,
                b'M' | b'R' | b'C' | b'T' | b'U' => counts.modified += 1,
                _ => {}
            }
        }
    }

    counts
}

/// Compare local branch with remote tracking branch
fn get_branch_sync(repo_path: &Path) -> Result<BranchSyncStatus> {
    if !has_upstream(repo_path)? {
        return Ok(BranchSyncStatus::NoRemote);
    }

    let Some((ahead, behind)) = ahead_behind(repo_path)? else {
        return Ok(BranchSyncStatus::NoRemote);
    };

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

/// Initialize a new git repository at the given path.
pub fn init_repo(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).context("Failed to create repository directory")?;
    run_git(path, &["init"]).context("Failed to initialize git repository")?;
    Ok(())
}

/// Stage the listed files and create a commit.
pub fn add_and_commit(repo_path: &Path, files: &[&str], message: &str) -> Result<()> {
    for file in files {
        run_git(repo_path, &["add", file]).with_context(|| format!("Failed to stage {}", file))?;
    }

    let output = if has_git_identity(repo_path)? {
        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git")?
    } else {
        let user_name =
            git_config_value(repo_path, "user.name")?.unwrap_or_else(|| "instantCLI".into());
        let user_email = git_config_value(repo_path, "user.email")?
            .unwrap_or_else(|| "instant@localhost".into());

        Command::new("git")
            .args([
                "-c",
                &format!("user.name={user_name}"),
                "-c",
                &format!("user.email={user_email}"),
                "commit",
                "-m",
                message,
            ])
            .current_dir(repo_path)
            .output()
            .context("Failed to execute git")?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git commit failed: {}", stderr.trim());
    }

    Ok(())
}

/// Check if a path is a git repository
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
        || Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) -> Result<String> {
        run_git(dir, args)
    }

    fn init_repo_with_commit(path: &Path) -> Result<()> {
        fs::create_dir_all(path)?;
        git(path, &["init"])?;
        git(
            path,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        )?;
        Ok(())
    }

    #[test]
    fn checkout_branch_creates_local_tracking_branch_from_origin() -> Result<()> {
        let temp = TempDir::new()?;
        let remote = temp.path().join("remote");
        let work = temp.path().join("work");
        let clone = temp.path().join("clone");

        fs::create_dir_all(&remote)?;
        let output = Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .context("Failed to init bare remote")?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to init bare remote: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        init_repo_with_commit(&work)?;
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        )?;
        git(&work, &["branch", "-M", "main"])?;
        git(&work, &["push", "-u", "origin", "main"])?;
        git(&work, &["checkout", "-b", "feature"])?;
        fs::write(work.join("feature.txt"), "feature")?;
        git(&work, &["add", "feature.txt"])?;
        git(
            &work,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "feature",
            ],
        )?;
        git(&work, &["push", "-u", "origin", "feature"])?;

        clone_repo(remote.to_str().unwrap(), &clone, Some("main"), None)?;
        fetch_branch(&clone, "feature")?;
        checkout_branch(&clone, "feature")?;

        assert_eq!(current_branch(&clone)?, "feature");
        Ok(())
    }

    #[test]
    fn clone_repo_treats_zero_depth_as_full_clone() -> Result<()> {
        let temp = TempDir::new()?;
        let source = temp.path().join("source");
        let clone = temp.path().join("clone");

        init_repo_with_commit(&source)?;
        fs::write(source.join("tracked.txt"), "tracked")?;
        git(&source, &["add", "tracked.txt"])?;
        git(
            &source,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "tracked",
            ],
        )?;

        clone_repo(source.to_str().unwrap(), &clone, None, Some(0))?;

        assert!(clone.join("tracked.txt").exists());
        Ok(())
    }

    #[test]
    fn fetch_branch_uses_branch_upstream_remote_before_origin() -> Result<()> {
        let temp = TempDir::new()?;
        let upstream = temp.path().join("upstream");
        let origin = temp.path().join("origin");
        let work = temp.path().join("work");
        let clone = temp.path().join("clone");

        for remote in [&upstream, &origin] {
            fs::create_dir_all(remote)?;
            let output = Command::new("git")
                .args(["init", "--bare"])
                .arg(remote)
                .output()
                .context("Failed to init bare remote")?;
            if !output.status.success() {
                anyhow::bail!(
                    "Failed to init bare remote: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
        }

        init_repo_with_commit(&work)?;
        git(
            &work,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        )?;
        git(
            &work,
            &["remote", "add", "upstream", upstream.to_str().unwrap()],
        )?;
        git(&work, &["branch", "-M", "main"])?;
        git(&work, &["push", "-u", "origin", "main"])?;
        git(&work, &["checkout", "-b", "feature"])?;
        fs::write(work.join("feature.txt"), "feature")?;
        git(&work, &["add", "feature.txt"])?;
        git(
            &work,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "feature",
            ],
        )?;
        git(&work, &["push", "-u", "upstream", "feature"])?;

        clone_repo(origin.to_str().unwrap(), &clone, Some("main"), None)?;
        git(
            &clone,
            &["remote", "add", "upstream", upstream.to_str().unwrap()],
        )?;
        git(&clone, &["config", "branch.feature.remote", "upstream"])?;
        git(
            &clone,
            &["config", "branch.feature.merge", "refs/heads/feature"],
        )?;

        fetch_branch(&clone, "feature")?;

        assert!(remote_branch_exists_on(&clone, "upstream", "feature")?);
        Ok(())
    }

    #[test]
    fn checkout_branch_uses_branch_upstream_remote_before_origin() -> Result<()> {
        let temp = TempDir::new()?;
        let upstream = temp.path().join("upstream");
        let origin = temp.path().join("origin");
        let work = temp.path().join("work");
        let clone = temp.path().join("clone");

        for remote in [&upstream, &origin] {
            fs::create_dir_all(remote)?;
            let output = Command::new("git")
                .args(["init", "--bare"])
                .arg(remote)
                .output()
                .context("Failed to init bare remote")?;
            if !output.status.success() {
                anyhow::bail!(
                    "Failed to init bare remote: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
        }

        init_repo_with_commit(&work)?;
        git(
            &work,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        )?;
        git(
            &work,
            &["remote", "add", "upstream", upstream.to_str().unwrap()],
        )?;
        git(&work, &["branch", "-M", "main"])?;
        git(&work, &["push", "-u", "origin", "main"])?;
        git(&work, &["checkout", "-b", "feature"])?;
        fs::write(work.join("feature.txt"), "feature")?;
        git(&work, &["add", "feature.txt"])?;
        git(
            &work,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "feature",
            ],
        )?;
        git(&work, &["push", "-u", "upstream", "feature"])?;

        clone_repo(origin.to_str().unwrap(), &clone, Some("main"), None)?;
        git(
            &clone,
            &["remote", "add", "upstream", upstream.to_str().unwrap()],
        )?;
        git(&clone, &["config", "branch.feature.remote", "upstream"])?;
        git(
            &clone,
            &["config", "branch.feature.merge", "refs/heads/feature"],
        )?;

        fetch_branch(&clone, "feature")?;
        checkout_branch(&clone, "feature")?;

        assert_eq!(current_branch(&clone)?, "feature");
        let upstream = git(&clone, &["rev-parse", "--abbrev-ref", "feature@{u}"])?;
        assert_eq!(upstream, "upstream/feature");
        Ok(())
    }

    #[test]
    fn stash_local_changes_preserves_dirty_worktree() -> Result<()> {
        let temp = TempDir::new()?;
        let repo = temp.path().join("repo");

        init_repo_with_commit(&repo)?;
        fs::write(repo.join("tracked.txt"), "tracked")?;
        git(&repo, &["add", "tracked.txt"])?;
        git(
            &repo,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "tracked",
            ],
        )?;

        fs::write(repo.join("tracked.txt"), "modified")?;
        fs::write(repo.join("untracked.txt"), "new")?;

        assert!(has_local_changes(&repo)?);
        assert!(stash_local_changes(&repo, "Auto-stash by instantCLI")?);
        assert!(!has_local_changes(&repo)?);

        let stash_list = git(&repo, &["stash", "list"])?;
        assert!(stash_list.contains("Auto-stash by instantCLI"));

        Ok(())
    }

    #[test]
    fn clean_and_pull_removes_ignored_files() -> Result<()> {
        let temp = TempDir::new()?;
        let remote = temp.path().join("remote");
        let work = temp.path().join("work");
        let clone = temp.path().join("clone");

        fs::create_dir_all(&remote)?;
        let output = Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .context("Failed to init bare remote")?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to init bare remote: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        init_repo_with_commit(&work)?;
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        )?;
        git(&work, &["branch", "-M", "main"])?;
        fs::write(work.join(".gitignore"), "ignored.log\n")?;
        fs::write(work.join("tracked.txt"), "tracked")?;
        git(&work, &["add", ".gitignore", "tracked.txt"])?;
        git(
            &work,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "tracked files",
            ],
        )?;
        git(&work, &["push", "-u", "origin", "main"])?;

        clone_repo(remote.to_str().unwrap(), &clone, Some("main"), None)?;

        fs::write(clone.join("ignored.log"), "ignored")?;
        fs::write(clone.join("untracked.txt"), "untracked")?;
        fs::write(clone.join("tracked.txt"), "modified")?;

        clean_and_pull(&clone)?;

        assert!(!clone.join("ignored.log").exists());
        assert!(!clone.join("untracked.txt").exists());
        assert_eq!(fs::read_to_string(clone.join("tracked.txt"))?, "tracked");
        Ok(())
    }

    #[test]
    fn default_push_readiness_reports_no_remote_for_local_repo() -> Result<()> {
        let temp = TempDir::new()?;
        let repo = temp.path().join("repo");

        init_repo_with_commit(&repo)?;

        assert_eq!(
            default_push_readiness(&repo)?,
            DefaultPushReadiness::NoRemote
        );
        Ok(())
    }

    #[test]
    fn default_push_readiness_reports_no_upstream_for_simple_push() -> Result<()> {
        let temp = TempDir::new()?;
        let remote = temp.path().join("remote");
        let work = temp.path().join("work");

        fs::create_dir_all(&remote)?;
        let output = Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .context("Failed to init bare remote")?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to init bare remote: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        init_repo_with_commit(&work)?;
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        )?;
        git(&work, &["branch", "-M", "main"])?;

        assert_eq!(
            default_push_readiness(&work)?,
            DefaultPushReadiness::NoUpstream {
                branch: "main".to_string()
            }
        );
        Ok(())
    }

    #[test]
    fn default_push_readiness_allows_current_push_without_upstream() -> Result<()> {
        let temp = TempDir::new()?;
        let remote = temp.path().join("remote");
        let work = temp.path().join("work");

        fs::create_dir_all(&remote)?;
        let output = Command::new("git")
            .args(["init", "--bare"])
            .arg(&remote)
            .output()
            .context("Failed to init bare remote")?;
        if !output.status.success() {
            anyhow::bail!(
                "Failed to init bare remote: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        init_repo_with_commit(&work)?;
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        )?;
        git(&work, &["branch", "-M", "main"])?;
        git(&work, &["config", "push.default", "current"])?;

        assert_eq!(default_push_readiness(&work)?, DefaultPushReadiness::Ready);
        Ok(())
    }

    #[test]
    fn repo_status_counts_porcelain_changes() -> Result<()> {
        let temp = TempDir::new()?;
        let repo = temp.path().join("repo");

        init_repo_with_commit(&repo)?;
        fs::write(repo.join("tracked.txt"), "one")?;
        git(&repo, &["add", "tracked.txt"])?;
        git(
            &repo,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "tracked",
            ],
        )?;

        fs::write(repo.join("tracked.txt"), "two")?;
        fs::write(repo.join("new.txt"), "new")?;

        let status = get_repo_status(&repo)?;
        assert!(!status.working_dir_clean);
        assert_eq!(status.file_counts.modified, 1);
        assert_eq!(status.file_counts.untracked, 1);

        Ok(())
    }

    #[test]
    fn repo_status_recurses_untracked_directories() -> Result<()> {
        let temp = TempDir::new()?;
        let repo = temp.path().join("repo");

        init_repo_with_commit(&repo)?;
        fs::create_dir_all(repo.join("nested"))?;
        fs::write(repo.join("nested/one.txt"), "one")?;
        fs::write(repo.join("nested/two.txt"), "two")?;

        let status = get_repo_status(&repo)?;
        assert_eq!(status.file_counts.untracked, 2);

        Ok(())
    }

    #[test]
    fn add_and_commit_prefers_repo_git_identity_with_fallback() -> Result<()> {
        let temp = TempDir::new()?;
        let repo = temp.path().join("repo");

        init_repo(&repo)?;
        git(&repo, &["config", "user.name", "Configured User"])?;
        git(&repo, &["config", "user.email", "configured@example.com"])?;
        fs::write(repo.join("tracked.txt"), "tracked")?;

        add_and_commit(&repo, &["tracked.txt"], "configured identity commit")?;

        let author_name = git(&repo, &["log", "-1", "--pretty=%an"])?;
        let author_email = git(&repo, &["log", "-1", "--pretty=%ae"])?;
        assert_eq!(author_name, "Configured User");
        assert_eq!(author_email, "configured@example.com");

        Ok(())
    }
}
