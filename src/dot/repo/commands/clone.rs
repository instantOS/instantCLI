use crate::dot::config::{DotfileConfig, extract_repo_name};
use crate::dot::db::Database;
use crate::dot::git::add_repo as git_clone_repo;
use crate::dot::repo::DotfileRepositoryManager;
use crate::ui::Level;
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;
use anyhow::Result;

use super::apply::apply_all_repos;

/// Resolve repository name from provided name, metadata, or URL
fn resolve_repo_name(url: &str, name: Option<&str>) -> String {
    name.map(|s| s.to_string())
        .or_else(|| {
            // For local paths, try to read name from instantdots.toml
            let path = std::path::Path::new(url);
            if path.exists() {
                let canonical = path.canonicalize().ok()?;
                crate::dot::meta::read_meta(&canonical)
                    .ok()
                    .map(|meta| meta.name)
            } else {
                None
            }
        })
        .unwrap_or_else(|| extract_repo_name(url))
}

/// Configure an external (yadm/stow) repository after cloning
fn configure_external_repo(
    config: &mut DotfileConfig,
    repo_name: &str,
    read_only: bool,
) -> Result<()> {
    emit(
        Level::Info,
        "dot.repo.clone.external",
        &format!(
            "{} Detected external dotfile repository (Yadm/Stow compatible)",
            char::from(NerdFont::Info)
        ),
        None,
    );

    for repo in &mut config.repos {
        if repo.name == repo_name {
            repo.active_subdirectories = Some(vec![".".to_string()]);
            repo.metadata = Some(crate::dot::types::RepoMetaData {
                name: repo_name.to_string(),
                author: None,
                description: None,
                read_only: if read_only { Some(true) } else { None },
                dots_dirs: vec![".".to_string()],
                default_active_subdirs: None,
                units: vec![],
            });
            break;
        }
    }
    config.save(None)
}

/// Check if repository metadata requests read-only mode and update config
fn handle_read_only_metadata(
    config: &mut DotfileConfig,
    db: &Database,
    repo_name: &str,
) -> Result<()> {
    if let Ok(local_repo) = DotfileRepositoryManager::new(config, db).get_repository_info(repo_name)
        && let Some(true) = local_repo.meta.read_only
    {
        emit(
            Level::Info,
            "dot.repo.clone.read_only",
            &format!(
                "{} Repository requested read-only mode. Marking as read-only.",
                char::from(NerdFont::Info)
            ),
            None,
        );
        for repo in &mut config.repos {
            if repo.name == repo_name {
                repo.read_only = true;
                break;
            }
        }
        config.save(None)?;
    }
    Ok(())
}

/// Options for cloning a repository
pub struct CloneOptions<'a> {
    pub url: &'a str,
    pub name: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub read_only: bool,
    pub force_write: bool,
    pub debug: bool,
}

/// Clone a new repository
pub fn clone_repository(
    config: &mut DotfileConfig,
    db: &Database,
    opts: CloneOptions<'_>,
) -> Result<()> {
    if opts.read_only && opts.force_write {
        return Err(anyhow::anyhow!(
            "Cannot use both --read-only and --force-write flags at the same time"
        ));
    }

    let repo_name = resolve_repo_name(opts.url, opts.name);

    let repo_config = crate::dot::config::Repo {
        url: opts.url.to_string(),
        name: repo_name.clone(),
        branch: opts.branch.map(|s| s.to_string()),
        active_subdirectories: None,
        enabled: true,
        read_only: opts.read_only,
        metadata: None,
    };

    config.add_repo(repo_config.clone(), None)?;

    emit(
        Level::Success,
        "dot.repo.clone.added",
        &format!(
            "{} Cloning repository '{}' from {}",
            char::from(NerdFont::Check),
            repo_name,
            opts.url
        ),
        None,
    );

    match git_clone_repo(config, repo_config, opts.debug) {
        Ok(path) => {
            emit(
                Level::Info,
                "dot.repo.clone.path",
                &format!(
                    "{} Cloned to: {}",
                    char::from(NerdFont::Folder),
                    path.display()
                ),
                None,
            );

            // Detect and configure external (yadm/stow) repos
            if !path.join("instantdots.toml").exists() {
                configure_external_repo(config, &repo_name, opts.read_only)?;
            }

            // Apply dotfiles
            emit(
                Level::Info,
                "dot.repo.clone.apply",
                &format!(
                    "{} Applying dotfiles from new repository...",
                    char::from(NerdFont::Info)
                ),
                None,
            );
            if let Err(e) = apply_all_repos(config, db) {
                emit(
                    Level::Warn,
                    "dot.repo.clone.apply_failed",
                    &format!(
                        "{} Failed to apply dotfiles: {e}",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
            }

            // Handle read-only metadata request
            if !opts.read_only && !opts.force_write {
                handle_read_only_metadata(config, db, &repo_name)?;
            }
        }
        Err(e) => {
            emit(
                Level::Error,
                "dot.repo.clone.failed",
                &format!(
                    "{} Failed to clone repository: {e}",
                    char::from(NerdFont::CrossCircle)
                ),
                None,
            );
            config.remove_repo(&repo_name, None)?;
            return Err(e);
        }
    }

    Ok(())
}
