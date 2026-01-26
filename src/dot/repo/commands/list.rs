use crate::common::TildePath;
use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::RepositoryManager;
use crate::ui::prelude::*;
use crate::ui::Level;
use anyhow::Result;
use colored::*;

/// Get local path in tilde notation for a repository
fn get_local_path_tilde(
    config: &Config,
    repo_manager: &RepositoryManager,
    repo_name: &str,
) -> String {
    repo_manager
        .get_repository_info(repo_name)
        .map(|local_repo| {
            let path = local_repo.local_path(config).unwrap_or_default();
            let tilde_path = TildePath::new(path.clone());
            tilde_path
                .to_tilde_string()
                .unwrap_or_else(|_| path.display().to_string())
        })
        .unwrap_or_else(|_| "Not found".to_string())
}

/// List all configured repositories
pub(super) fn list_repositories(config: &Config, db: &Database) -> Result<()> {
    if config.repos.is_empty() {
        match get_output_format() {
            OutputFormat::Json => {
                let data = serde_json::json!({
                    "repos": [],
                    "count": 0
                });
                emit(
                    Level::Info,
                    "dot.repo.list",
                    "No repositories configured",
                    Some(data),
                );
            }
            OutputFormat::Text => {
                println!("No repositories configured.");
            }
        }
        return Ok(());
    }

    let repo_manager = RepositoryManager::new(config, db);

    match get_output_format() {
        OutputFormat::Json => {
            let repos_data: Vec<_> = config
                .repos
                .iter()
                .map(|repo_config| {
                    let local_path = get_local_path_tilde(config, &repo_manager, &repo_config.name);

                    serde_json::json!({
                        "name": repo_config.name,
                        "url": repo_config.url,
                        "branch": repo_config.branch,
                        "enabled": repo_config.enabled,
                        "active_subdirectories": config
                            .resolve_active_subdirs(repo_config)
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>(),
                        "read_only": repo_config.read_only,
                        "local_path": local_path
                    })
                })
                .collect();

            let data = serde_json::json!({
                "repos": repos_data,
                "count": config.repos.len()
            });

            emit(
                Level::Info,
                "dot.repo.list",
                &format!("Configured repositories: {}", config.repos.len()),
                Some(data),
            );
        }
        OutputFormat::Text => {
            println!("Configured repositories:");
            let total_repos = config.repos.len();

            for (index, repo_config) in config.repos.iter().enumerate() {
                // Priority: P1 = highest (first repo), P2 = second highest, etc.
                let priority = index + 1;
                let priority_label = format!("[P{}]", priority).bright_purple().bold();

                let status = if repo_config.enabled {
                    "enabled".green()
                } else {
                    "disabled".yellow()
                };

                let read_only = if repo_config.read_only {
                    " [read-only]".yellow()
                } else {
                    "".clear()
                };

                let branch_info = repo_config
                    .branch
                    .as_deref()
                    .map(|b| format!(" ({b})"))
                    .unwrap_or_default();

                // Show subdir priority when multiple active subdirs
                let effective_active_subdirs = config.resolve_active_subdirs(repo_config);
                let defaults_disabled = repo_config.active_subdirectories.is_none()
                    && repo_manager
                        .get_repository_info(&repo_config.name)
                        .ok()
                        .map(|local_repo| {
                            local_repo
                                .meta
                                .default_active_subdirs
                                .as_ref()
                                .map(|dirs| dirs.is_empty())
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);
                let active_subdirs = if effective_active_subdirs.is_empty() {
                    if defaults_disabled {
                        "(disabled by defaults)".to_string()
                    } else {
                        let repo_path = config.repos_path().join(&repo_config.name);
                        if repo_path.join("instantdots.toml").exists()
                            || repo_config.metadata.is_some()
                        {
                            "(none configured)".to_string()
                        } else {
                            "(none detected)".to_string()
                        }
                    }
                } else {
                    let subdirs = effective_active_subdirs.join(", ");
                    if effective_active_subdirs.len() > 1 {
                        // Show priority order for multiple subdirs
                        format!("{} (priority: first=highest)", subdirs)
                    } else {
                        subdirs
                    }
                };

                // Get local path in tilde notation
                let local_path = get_local_path_tilde(config, &repo_manager, &repo_config.name);

                // Overall priority hint
                let priority_hint = if priority == 1 && total_repos > 1 {
                    format!(" {}", "(highest priority)".dimmed())
                } else if priority == total_repos && total_repos > 1 {
                    format!(" {}", "(lowest priority)".dimmed())
                } else {
                    String::new()
                };

                println!(
                    "  {} {}{} - {} [{}]{}{}",
                    priority_label,
                    repo_config.name.cyan(),
                    branch_info,
                    repo_config.url,
                    status,
                    read_only,
                    priority_hint
                );
                println!("    Local path: {}", local_path.dimmed());
                println!("    Active subdirs: {active_subdirs}");
            }
        }
    }

    Ok(())
}
