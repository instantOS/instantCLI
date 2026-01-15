//! Dotfile discovery - scanning repos to find dotfiles and their sources.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::dot::config::Config;
use crate::dot::localrepo::LocalRepo;
use crate::dot::override_config::DotfileSource;

/// A dotfile with all its available sources across repos.
#[derive(Clone)]
pub struct DiscoveredDotfile {
    pub target_path: PathBuf,
    pub display_path: String,
    pub sources: Vec<DotfileSource>,
}

/// Filter for dotfile discovery.
#[derive(Clone, Copy, Default)]
pub enum DiscoveryFilter {
    /// All dotfiles.
    #[default]
    All,
    /// Only dotfiles with multiple sources (alternatives).
    WithAlternatives,
}

/// Find dotfiles in a directory, optionally filtering by those with alternatives.
pub fn discover_dotfiles(
    config: &Config,
    dir_path: &Path,
    filter: DiscoveryFilter,
) -> Result<Vec<DiscoveredDotfile>> {
    let home = home_dir();
    let mut sources_by_target: HashMap<PathBuf, Vec<DotfileSource>> = HashMap::new();

    for repo_config in &config.repos {
        if !repo_config.enabled {
            continue;
        }

        let local_repo = match LocalRepo::new(config, repo_config.name.clone()) {
            Ok(repo) => repo,
            Err(_) => continue,
        };

        for dotfile_dir in &local_repo.dotfile_dirs {
            for entry in WalkDir::new(&dotfile_dir.path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    !e.path().to_string_lossy().contains("/.git/") && e.file_type().is_file()
                })
            {
                let source_path = entry.path().to_path_buf();
                let relative_path = match source_path.strip_prefix(&dotfile_dir.path) {
                    Ok(rel) => rel,
                    Err(_) => continue,
                };
                let target_path = home.join(relative_path);

                if !target_path.starts_with(dir_path) {
                    continue;
                }

                let subdir_name = dotfile_dir
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                sources_by_target
                    .entry(target_path)
                    .or_default()
                    .push(DotfileSource {
                        repo_name: repo_config.name.clone(),
                        subdir_name,
                        source_path,
                    });
            }
        }
    }

    let min_sources = match filter {
        DiscoveryFilter::All => 1,
        DiscoveryFilter::WithAlternatives => 2,
    };

    let mut results: Vec<DiscoveredDotfile> = sources_by_target
        .into_iter()
        .filter(|(_, sources)| sources.len() >= min_sources)
        .map(|(target_path, sources)| DiscoveredDotfile {
            display_path: to_display_path(&target_path),
            target_path,
            sources,
        })
        .collect();

    results.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    Ok(results)
}

/// Get all writable repo/subdir destinations.
pub fn get_destinations(config: &Config) -> Vec<DotfileSource> {
    config
        .repos
        .iter()
        .filter(|r| r.enabled && !r.read_only)
        .flat_map(|repo| {
            repo.active_subdirectories
                .iter()
                .map(|subdir| DotfileSource {
                    repo_name: repo.name.clone(),
                    subdir_name: subdir.clone(),
                    source_path: config.repos_path().join(&repo.name).join(subdir),
                })
        })
        .collect()
}

pub fn home_dir() -> PathBuf {
    PathBuf::from(shellexpand::tilde("~").to_string())
}

pub fn to_display_path(path: &Path) -> String {
    path.strip_prefix(home_dir())
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| path.display().to_string())
}
