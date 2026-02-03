//! Dotfile discovery - scanning repos to find dotfiles and their sources.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::dot::config::Config;
use crate::dot::override_config::{DotfileSource, OverrideConfig};
use crate::dot::sources;
use crate::ui::nerd_font::NerdFont;
use crate::ui::{Level, emit};
use anyhow::Result;
use colored::Colorize;

/// A dotfile with all its available sources across repos.
#[derive(Clone)]
pub struct DiscoveredDotfile {
    pub target_path: PathBuf,
    pub display_path: String,
    pub sources: Vec<DotfileSource>,
    /// Whether this file has an override set (even if only 1 source exists)
    pub has_override: bool,
    /// Override details (even if source is missing)
    pub override_status: Option<OverrideStatus>,
    /// Default source when no override is set (priority-based)
    pub default_source: Option<DotfileSource>,
}

#[derive(Clone)]
pub struct OverrideStatus {
    pub source: DotfileSource,
    pub exists: bool,
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
    let home = sources::home_dir();
    let sources_by_target = sources::list_sources_by_target_in_dir(config, dir_path)?;

    // Load overrides to include files with explicit overrides even if they only have 1 source
    let overrides = OverrideConfig::load().unwrap_or_default();
    let mut override_lookup = HashMap::new();
    for override_entry in &overrides.overrides {
        override_lookup.insert(
            override_entry.target_path.as_path().to_path_buf(),
            (
                override_entry.source_repo.clone(),
                override_entry.source_subdir.clone(),
            ),
        );
    }
    let overridden_paths: HashSet<PathBuf> = override_lookup.keys().cloned().collect();

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
            let override_status =
                override_lookup
                    .get(&target_path)
                    .map(|(repo_name, subdir_name)| {
                        if let Some(source) = sources
                            .iter()
                            .find(|s| s.repo_name == *repo_name && s.subdir_name == *subdir_name)
                        {
                            OverrideStatus {
                                source: source.clone(),
                                exists: true,
                            }
                        } else {
                            let relative_path =
                                target_path.strip_prefix(&home).unwrap_or(&target_path);
                            let source_path = config
                                .repos_path()
                                .join(repo_name)
                                .join(subdir_name)
                                .join(relative_path);
                            OverrideStatus {
                                source: DotfileSource {
                                    repo_name: repo_name.clone(),
                                    subdir_name: subdir_name.clone(),
                                    source_path,
                                },
                                exists: false,
                            }
                        }
                    });
            let default_source = super::default_source_for(&sources);
            DiscoveredDotfile {
                display_path: to_display_path(&target_path),
                has_override,
                override_status,
                default_source,
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

        let active_subdirs = config.resolve_active_subdirs(repo);

        for subdir in &active_subdirs {
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

pub fn to_display_path(path: &Path) -> String {
    path.strip_prefix(sources::home_dir())
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| path.display().to_string())
}
