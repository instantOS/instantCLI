use crate::common;
use crate::common::git;
use crate::dot::config;
use crate::dot::db::DotFileType;
use crate::dot::dotfilerepo as repo_mod;
use crate::dot::get_all_dotfiles;
use anyhow::{Context, Result};
use git2::Repository;
use std::path::PathBuf;

/// Get the dotfile directory name for a dotfile
pub fn get_dotfile_dir_name(dotfile: &crate::dot::Dotfile, cfg: &config::Config) -> String {
    // Find which repository this dotfile comes from
    for repo_config in &cfg.repos {
        let repo_path = cfg.repos_path().join(&repo_config.name);
        if dotfile.source_path.starts_with(&repo_path) {
            // Extract the dotfile directory name from the source path
            // Source path format: {repo_path}/{dotfile_dir}/{relative_path}
            if let Ok(relative) = dotfile.source_path.strip_prefix(&repo_path)
                && let Some(dotfile_dir) = relative.components().next()
            {
                return dotfile_dir.as_os_str().to_string_lossy().to_string();
            }
            return "dots".to_string(); // default
        }
    }
    "unknown".to_string()
}

/// Get the repository name for a dotfile (improved version)
pub fn get_repo_name_for_dotfile(
    dotfile: &crate::dot::Dotfile,
    cfg: &config::Config,
) -> crate::dot::RepoName {
    // Find which repository this dotfile comes from
    for repo_config in &cfg.repos {
        if dotfile
            .source_path
            .starts_with(cfg.repos_path().join(&repo_config.name))
        {
            return crate::dot::RepoName::new(repo_config.name.clone());
        }
    }
    crate::dot::RepoName::new("unknown".to_string())
}

pub fn add_repo(config: &mut config::Config, repo: config::Repo, debug: bool) -> Result<PathBuf> {
    let base = config.repos_path();

    let repo_dir_name = repo.name.clone();

    let target = base.join(repo_dir_name);

    let mut skip_clone = false;
    if target.exists() {
        if Repository::open(&target).is_ok() {
            if debug {
                eprintln!(
                    "Destination '{}' already exists and is a git repository. Skipping clone.",
                    target.display()
                );
            }
            skip_clone = true;
        } else if target.read_dir()?.next().is_some() {
            return Err(anyhow::anyhow!(
                "Destination '{}' already exists and is not empty or a git repository",
                target.display()
            ));
        }
    }

    // For local paths, canonicalize to absolute path and disable shallow clone
    let local_path = std::path::Path::new(&repo.url);
    let (clone_url, depth) = if local_path.exists() {
        let canonical = local_path
            .canonicalize()
            .with_context(|| format!("Failed to resolve path: {}", repo.url))?;
        (canonical.to_string_lossy().to_string(), None)
    } else {
        (repo.url.clone(), Some(config.clone_depth as i32))
    };

    if !skip_clone {
        let pb = common::progress::create_spinner(format!("Cloning {}...", clone_url));

        git::clone_repo(&clone_url, &target, repo.branch.as_deref(), depth)
            .context("Failed to clone repository")?;

        common::progress::finish_spinner_with_success(pb, format!("Cloned {}", clone_url));
    } else if let Some(branch) = repo.branch.as_deref() {
        // If we reused an existing repo, try to ensure the correct branch is checked out
        if let Ok(mut repo_instance) = Repository::open(&target) {
            if let Err(e) = git::checkout_branch(&mut repo_instance, branch) {
                if debug {
                    eprintln!("Warning: Failed to checkout branch '{}': {}", branch, e);
                }
            } else if debug {
                eprintln!("Checked out branch '{}'", branch);
            }
        }
    }

    // Create missing dots directories (git doesn't track empty directories)
    if let Ok(meta) = crate::dot::meta::read_meta(&target) {
        for dots_dir in &meta.dots_dirs {
            let dots_path = target.join(dots_dir);
            if !dots_path.exists() {
                std::fs::create_dir_all(&dots_path).with_context(|| {
                    format!("Failed to create dots directory: {}", dots_path.display())
                })?;
                if debug {
                    eprintln!("Created missing dots directory: {}", dots_path.display());
                }
            }
        }
    }

    // Validate metadata
    let dotfile_repo = match repo_mod::DotfileRepo::new(config, repo.name.clone()) {
        Ok(repo) => repo,
        Err(e) => {
            if !target.join("instantdots.toml").exists() {
                // This might be an external repo (yadm/stow) that will be configured
                // by the caller (clone_repository).
                if debug {
                    eprintln!("Repo has no metadata. Skipping validation and hash registration.");
                }
                return Ok(target);
            }
            return Err(e);
        }
    };

    let meta = &dotfile_repo.meta;

    if debug {
        eprintln!(
            "Repo {} identified as dot repo '{}' - {}",
            dotfile_repo.url,
            meta.name,
            meta.description.as_deref().unwrap_or("")
        );
    }

    // Initialize database with source file hashes to prevent false "modified" status
    // when identical files already exist in the home directory
    if let Ok(db) = crate::dot::db::Database::new(config.database_path().to_path_buf())
        && let Ok(dotfiles) = get_all_dotfiles(config, &db)
    {
        for (_, dotfile) in dotfiles {
            // Only register hashes for dotfiles from this repository
            if dotfile.source_path.starts_with(&target) {
                // Register the source file hash with source_file=true
                if let Ok(source_hash) =
                    crate::dot::dotfile::Dotfile::compute_hash(&dotfile.source_path)
                {
                    db.add_hash(&source_hash, &dotfile.source_path, DotFileType::SourceFile)?; // source_file=true

                    // If the target file exists and has the same content,
                    // register it with source_file=false
                    if dotfile.target_path.exists()
                        && let Ok(target_hash) =
                            crate::dot::dotfile::Dotfile::compute_hash(&dotfile.target_path)
                        && target_hash == source_hash
                    {
                        db.add_hash(&target_hash, &dotfile.target_path, DotFileType::TargetFile)?; // source_file=false
                    }
                }
            }
        }
    }

    Ok(target)
}

pub fn update_all(
    cfg: &config::Config,
    debug: bool,
    db: &crate::dot::db::Database,
    should_apply: bool,
) -> Result<()> {
    let repos = cfg.repos.clone();
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    let mut any_failed = false;

    for repo in repos.iter() {
        let dotfile_repo = repo_mod::DotfileRepo::new(cfg, repo.name.clone())?;
        if let Err(e) = dotfile_repo.update(cfg, debug) {
            eprintln!("Failed to update {}:", repo.url);
            for (i, cause) in e.chain().enumerate() {
                if i == 0 {
                    eprintln!("  {}", cause);
                } else {
                    eprintln!("  Caused by: {}", cause);
                }
            }
            any_failed = true;
        }
    }

    if should_apply {
        crate::dot::operations::apply_all(cfg, db)?;
    }

    if any_failed {
        Err(anyhow::anyhow!(
            "One or more repositories failed to update (see error messages above for details)"
        ))
    } else {
        Ok(())
    }
}

/// Run a git command in the specified repository
pub fn run_git_command(repo_path: &std::path::Path, args: &[&str], debug: bool) -> Result<()> {
    if debug {
        println!("Running git {:?} in {}", args, repo_path.display());
    }

    let status = std::process::Command::new("git")
        .current_dir(repo_path)
        .args(args)
        .status()
        .context("Failed to execute git command")?;

    if !status.success() {
        return Err(anyhow::anyhow!(
            "Git command failed with status: {}",
            status
        ));
    }

    Ok(())
}

/// Run an interactive git command (connected to TTY)
pub fn run_interactive_git_command(
    repo_path: &std::path::Path,
    args: &[&str],
    debug: bool,
) -> Result<()> {
    if debug {
        println!(
            "Running interactive git {:?} in {}",
            args,
            repo_path.display()
        );
    }

    let status = std::process::Command::new("git")
        .current_dir(repo_path)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to execute interactive git command")?;

    if !status.success() {
        return Err(anyhow::anyhow!(
            "Git command failed with status: {}",
            status
        ));
    }

    Ok(())
}

/// Run git add for a specific file in a repository
pub fn git_add(
    repo_path: &std::path::Path,
    file_path: &std::path::Path,
    debug: bool,
) -> Result<()> {
    run_git_command(repo_path, &["add", &file_path.to_string_lossy()], debug)
}
