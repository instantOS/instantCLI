use crate::common;
use crate::common::git;
use crate::dot::config;
use crate::dot::db::DotFileType;
use crate::dot::get_all_dotfiles;
use crate::dot::localrepo as repo_mod;
use anyhow::{Context, Result};
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

    if target.exists() {
        return Err(anyhow::anyhow!(
            "Destination '{}' already exists",
            target.display()
        ));
    }

    let depth = config.clone_depth;

    let pb = common::progress::create_spinner(format!("Cloning {}...", repo.url));

    git::clone_repo(
        &repo.url,
        &target,
        repo.branch.as_deref(),
        Some(depth as i32),
    )
    .context("Failed to clone repository")?;

    pb.finish_with_message(format!("Cloned {}", repo.url));

    // Note: config addition is now handled by the caller (clone_repository function)

    // validate metadata but do not delete invalid clones; report their existence
    let local_repo = repo_mod::LocalRepo::new(config, repo.name.clone())?;
    let meta = &local_repo.meta;

    if debug {
        eprintln!(
            "Repo {} identified as dot repo '{}' - {}",
            local_repo.url,
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
        let local_repo = repo_mod::LocalRepo::new(cfg, repo.name.clone())?;
        if let Err(e) = local_repo.update(cfg, debug) {
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
