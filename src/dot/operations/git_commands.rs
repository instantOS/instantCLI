use crate::dot::config::{Config, Repo};
use crate::dot::git::repo_ops::{run_git_command, run_interactive_git_command};
use crate::dot::localrepo::LocalRepo;
use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::*;

/// Run git commit across all writable repositories
pub fn git_commit_all(config: &Config, args: &[String], debug: bool) -> Result<()> {
    let repos = config.get_writable_repos();

    if repos.is_empty() {
        println!("No writable repositories found.");
        return Ok(());
    }

    // First check if there are changes to commit in any repo
    let mut repos_with_changes: Vec<(&Repo, std::path::PathBuf)> = Vec::new();
    for repo in repos {
        let local_repo = LocalRepo::new(config, repo.name.clone())?;
        let repo_path = local_repo.local_path(config)?;

        // Check for changes (staged or unstaged)
        let status = std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["status", "--porcelain"])
            .output()?;

        if !status.stdout.is_empty() {
            repos_with_changes.push((repo, repo_path));
        }
    }

    if repos_with_changes.is_empty() {
        println!("No changes to commit in any writable repository.");
        return Ok(());
    }

    // Build the git commit command with any extra args
    let mut git_args = vec!["commit"];
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    git_args.extend(args_str);

    for (repo, repo_path) in repos_with_changes {
        println!(
            "\n{} Committing changes in '{}'...",
            char::from(NerdFont::Git),
            repo.name.cyan()
        );

        // We run interactively to let the user write the commit message
        if let Err(e) = run_interactive_git_command(&repo_path, &git_args, debug) {
            eprintln!("Failed to commit in {}: {}", repo.name, e);
        }
    }

    Ok(())
}

/// Run git push across all writable repositories
pub fn git_push_all(config: &Config, args: &[String], debug: bool) -> Result<()> {
    let repos = config.get_writable_repos();

    if repos.is_empty() {
        println!("No writable repositories found.");
        return Ok(());
    }

    // Build the git push command with any extra args
    let mut git_args = vec!["push"];
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    git_args.extend(args_str);

    for repo in repos {
        println!(
            "\n{} Pushing changes in '{}'...",
            char::from(NerdFont::Git),
            repo.name.cyan()
        );

        let local_repo = LocalRepo::new(config, repo.name.clone())?;
        let repo_path = local_repo.local_path(config)?;

        if let Err(e) = run_git_command(&repo_path, &git_args, debug) {
            eprintln!("Failed to push in {}: {}", repo.name, e);
        }
    }

    Ok(())
}

/// Get the current HEAD commit hash for a repository
fn get_head_commit(repo_path: &std::path::Path) -> Option<String> {
    std::process::Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        })
}

/// Run git pull across all writable repositories
/// Returns true if any repository successfully pulled new commits
pub fn git_pull_all(config: &Config, args: &[String], debug: bool) -> Result<bool> {
    let repos = config.get_writable_repos();

    if repos.is_empty() {
        println!("No writable repositories found.");
        return Ok(false);
    }

    // Build the git pull command with any extra args
    let mut git_args = vec!["pull"];
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    git_args.extend(args_str);

    let mut any_pulled_commits = false;

    for repo in repos {
        println!(
            "\n{} Pulling changes in '{}'...",
            char::from(NerdFont::Git),
            repo.name.cyan()
        );

        let local_repo = LocalRepo::new(config, repo.name.clone())?;
        let repo_path = local_repo.local_path(config)?;

        // Get HEAD before pull
        let head_before = get_head_commit(&repo_path);

        // Run git pull interactively (may need editor for merge commits, may fail with conflicts)
        if let Err(e) = run_interactive_git_command(&repo_path, &git_args, debug) {
            eprintln!("Failed to pull in {}: {}", repo.name, e);
            // Don't mark as having pulled commits if pull failed
            continue;
        }

        // Get HEAD after pull
        let head_after = get_head_commit(&repo_path);

        // Check if new commits were pulled
        if head_before != head_after {
            any_pulled_commits = true;
            if debug {
                println!(
                    "  {} New commits pulled (HEAD changed from {:?} to {:?})",
                    char::from(NerdFont::Check),
                    head_before,
                    head_after
                );
            }
        }
    }

    Ok(any_pulled_commits)
}

/// Run an arbitrary git command
pub fn git_run_any(config: &Config, args: &[String], debug: bool) -> Result<()> {
    // If no args provided, show help or error
    if args.is_empty() {
        return Err(anyhow::anyhow!("No git command provided"));
    }

    let repos = config.get_writable_repos();

    if repos.is_empty() {
        println!("No writable repositories found.");
        return Ok(());
    }

    let target_repo = if repos.len() == 1 {
        repos[0].clone()
    } else {
        // Let user choose a repo
        let items: Vec<crate::dot::types::RepoSelectItem> = repos
            .iter()
            .map(|&repo| crate::dot::types::RepoSelectItem { repo: repo.clone() })
            .collect();

        match FzfWrapper::builder()
            .prompt("Select repository to run git command in: ")
            .select(items)
            .map_err(|e| anyhow::anyhow!("Selection error: {}", e))?
        {
            FzfResult::Selected(item) => item.repo,
            FzfResult::Cancelled => return Ok(()),
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Err(anyhow::anyhow!("Unexpected selection result")),
        }
    };

    let local_repo = LocalRepo::new(config, target_repo.name.clone())?;
    let repo_path = local_repo.local_path(config)?;

    // We treat all custom commands as potentially needing interaction (e.g. status with pager, log, etc)
    // transforming Vec<String> to Vec<&str>
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    run_interactive_git_command(&repo_path, &args_str, debug)?;

    Ok(())
}
