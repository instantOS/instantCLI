//! Dotfile discovery - scanning repos to find dotfiles and their sources.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use colored::Colorize;
use walkdir::WalkDir;

use crate::dot::config::Config;
use crate::dot::localrepo::LocalRepo;
use crate::dot::override_config::{DotfileSource, OverrideConfig};
use crate::ui::nerd_font::NerdFont;
use crate::ui::{Level, emit};

/// A dotfile with all its available sources across repos.
#[derive(Clone)]
pub struct DiscoveredDotfile {
    pub target_path: PathBuf,
    pub display_path: String,
    pub sources: Vec<DotfileSource>,
    /// Whether this file has an override set (even if only 1 source exists)
    pub has_override: bool,
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

    // Load overrides to include files with explicit overrides even if they only have 1 source
    let overridden_paths: HashSet<PathBuf> = OverrideConfig::load()
        .map(|o| {
            o.overrides
                .iter()
                .map(|ov| ov.target_path.as_path().to_path_buf())
                .collect()
        })
        .unwrap_or_default();

    let mut results: Vec<DiscoveredDotfile> = sources_by_target
        .into_iter()
        .filter(|(target_path, sources)| {
            match filter {
                DiscoveryFilter::All => !sources.is_empty(),
                DiscoveryFilter::WithAlternatives => {
                    // Include if: has 2+ sources OR has an override set
                    sources.len() >= 2 || overridden_paths.contains(target_path)
                }
            }
        })
        .map(|(target_path, sources)| {
            let has_override = overridden_paths.contains(&target_path);
            DiscoveredDotfile {
                display_path: to_display_path(&target_path),
                has_override,
                target_path,
                sources,
            }
        })
        .collect();

    results.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    Ok(results)
}

/// Get all writable repo/subdir destinations.
/// Only includes subdirs that are in the repo's metadata (dots_dirs).
/// Warns if a subdir is in active_subdirectories but not in metadata.
pub fn get_destinations(config: &Config) -> Vec<DotfileSource> {
    // Track warnings to avoid duplicates within a session
    static WARNED_INVALID_SUBDIRS: OnceLock<std::sync::Mutex<HashSet<String>>> = OnceLock::new();
    let warned = WARNED_INVALID_SUBDIRS.get_or_init(|| std::sync::Mutex::new(HashSet::new()));

    let mut destinations = Vec::new();

    for repo in config.repos.iter().filter(|r| r.enabled && !r.read_only) {
        // Get the valid subdirs from metadata
        let valid_subdirs: HashSet<String> = if let Some(meta) = &repo.metadata {
            meta.dots_dirs.iter().cloned().collect()
        } else {
            // Try to read metadata from disk
            let repo_path = config.repos_path().join(&repo.name);
            match crate::dot::meta::read_meta(&repo_path) {
                Ok(meta) => meta.dots_dirs.iter().cloned().collect(),
                Err(_) => HashSet::new(), // No valid subdirs if metadata can't be read
            }
        };

        let active_subdirs = repo.active_subdirectories.as_deref().unwrap_or(&[]);

        for subdir in active_subdirs {
            if valid_subdirs.contains(subdir) {
                // Valid destination - in metadata
                destinations.push(DotfileSource {
                    repo_name: repo.name.clone(),
                    subdir_name: subdir.clone(),
                    source_path: config.repos_path().join(&repo.name).join(subdir),
                });
            } else {
                // Invalid - not in metadata, warn once per session
                let key = format!("{}:{}", repo.name, subdir);
                let should_warn = warned
                    .lock()
                    .map(|mut set| set.insert(key))
                    .unwrap_or(false);

                if should_warn {
                    emit(
                        Level::Warn,
                        "dot.destination.invalid_subdir",
                        &format!(
                            "{} Subdir '{}' in {} is enabled but not in repo metadata - not available as destination",
                            char::from(NerdFont::Warning),
                            subdir.yellow(),
                            repo.name.cyan()
                        ),
                        None,
                    );
                }
            }
        }
    }

    destinations
}

pub fn home_dir() -> PathBuf {
    PathBuf::from(shellexpand::tilde("~").to_string())
}

pub fn to_display_path(path: &Path) -> String {
    path.strip_prefix(home_dir())
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| path.display().to_string())
}
