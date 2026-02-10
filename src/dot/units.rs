//! Dotfile Units Module
//!
//! This module provides unit-aware modification detection. A "unit" is a directory
//! that should be treated atomically - if any file in the unit is modified by the user,
//! all files in that unit are treated as modified and won't be updated.
//!
//! Units can be defined in two places:
//! - Per-repository in `instantdots.toml`
//! - Globally in `~/.config/instant/dots.toml`
//!
//! The effective units are the union of both sources.

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::repo::DotfileRepositoryManager;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Find all unit paths that a target path belongs to (if any).
///
/// Returns units sorted by depth (more specific first), then lexicographically.
pub fn find_units_for_path(target_path: &Path, units: &[PathBuf]) -> Vec<PathBuf> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let mut matches = Vec::new();

    for unit in units {
        // Unit paths are relative to home, e.g. ".config/nvim"
        let unit_full_path = home.join(unit);
        if target_path.starts_with(&unit_full_path) {
            matches.push(unit.clone());
        }
    }

    matches.sort_by(|a, b| {
        let depth_cmp = b.components().count().cmp(&a.components().count());
        if depth_cmp == std::cmp::Ordering::Equal {
            a.to_string_lossy().cmp(&b.to_string_lossy())
        } else {
            depth_cmp
        }
    });
    matches.dedup();
    matches
}

fn normalize_unit_path(unit: &str) -> PathBuf {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    if unit.starts_with('~') {
        let expanded = PathBuf::from(shellexpand::tilde(unit).to_string());
        return expanded
            .strip_prefix(&home)
            .unwrap_or(&expanded)
            .to_path_buf();
    }

    let path = PathBuf::from(unit);
    if path.is_absolute() {
        return path.strip_prefix(&home).unwrap_or(&path).to_path_buf();
    }

    path
}

/// Get all effective units by combining global config units and repo-defined units.
///
/// Returns a deduplicated list of unit paths (relative to home directory).
pub fn get_all_units(config: &DotfileConfig, db: &Database) -> Result<Vec<PathBuf>> {
    let mut units_set: HashSet<PathBuf> = HashSet::new();

    // Add global config units
    for unit in &config.units {
        units_set.insert(normalize_unit_path(unit));
    }

    // Add repo-defined units
    let repo_manager = DotfileRepositoryManager::new(config, db);
    for repo_config in &config.repos {
        if !repo_config.enabled {
            continue;
        }
        if let Ok(local_repo) = repo_manager.get_repository_info(&repo_config.name) {
            for unit in &local_repo.meta.units {
                units_set.insert(normalize_unit_path(unit));
            }
        }
    }

    Ok(units_set.into_iter().collect())
}

#[derive(Debug, Default)]
pub struct UnitIndex {
    units_for_target: HashMap<PathBuf, Vec<PathBuf>>,
    modified_files_by_unit: HashMap<PathBuf, Vec<PathBuf>>,
}

#[derive(Debug, Clone)]
pub struct UnitStatus {
    pub unit_path: PathBuf,
    pub modified_files: Vec<PathBuf>,
}

impl UnitIndex {
    pub fn is_target_in_modified_unit(&self, target_path: &Path) -> bool {
        !self
            .modified_units_with_files_for_target(target_path)
            .is_empty()
    }

    pub fn unit_statuses_for_target(&self, target_path: &Path) -> Vec<UnitStatus> {
        let Some(units) = self.units_for_target.get(target_path) else {
            return Vec::new();
        };

        units
            .iter()
            .map(|unit_path| {
                let modified_files = self
                    .modified_files_by_unit
                    .get(unit_path)
                    .cloned()
                    .unwrap_or_default();
                UnitStatus {
                    unit_path: unit_path.clone(),
                    modified_files,
                }
            })
            .collect()
    }

    pub fn modified_units_with_files_for_target(
        &self,
        target_path: &Path,
    ) -> Vec<(PathBuf, Vec<PathBuf>)> {
        let Some(units) = self.units_for_target.get(target_path) else {
            return Vec::new();
        };

        units
            .iter()
            .filter_map(|unit_path| {
                self.modified_files_by_unit
                    .get(unit_path)
                    .map(|files| (unit_path.clone(), files.clone()))
            })
            .collect()
    }

    pub fn modified_units(&self) -> impl Iterator<Item = &PathBuf> {
        self.modified_files_by_unit.keys()
    }
}

/// Build a unit index for fast unit lookup and modification checks.
pub fn build_unit_index(
    all_dotfiles: &HashMap<PathBuf, Dotfile>,
    units: &[PathBuf],
    db: &Database,
) -> Result<UnitIndex> {
    let mut index = UnitIndex::default();

    if units.is_empty() {
        return Ok(index);
    }

    for dotfile in all_dotfiles.values() {
        let units_for_target = find_units_for_path(&dotfile.target_path, units);
        if !units_for_target.is_empty() {
            index
                .units_for_target
                .insert(dotfile.target_path.clone(), units_for_target.clone());

            if !dotfile.is_target_unmodified(db)? {
                for unit_path in units_for_target {
                    index
                        .modified_files_by_unit
                        .entry(unit_path)
                        .or_default()
                        .push(dotfile.target_path.clone());
                }
            }
        }
    }

    for files in index.modified_files_by_unit.values_mut() {
        files.sort();
        files.dedup();
    }

    Ok(index)
}

/// Compute which units have any modified files.
///
/// Returns a set of unit paths that contain at least one modified file.
/// This is useful for showing a single "unit skipped" message per unit.
pub fn get_modified_units(
    all_dotfiles: &HashMap<PathBuf, Dotfile>,
    units: &[PathBuf],
    db: &Database,
) -> Result<HashSet<PathBuf>> {
    let index = build_unit_index(all_dotfiles, units, db)?;
    Ok(index.modified_units().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_units_for_path_basic() {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let units = vec![
            PathBuf::from(".config/nvim"),
            PathBuf::from(".config/helix"),
        ];

        let nvim_init = home.join(".config/nvim/init.lua");
        assert_eq!(
            find_units_for_path(&nvim_init, &units),
            vec![PathBuf::from(".config/nvim")]
        );

        let bashrc = home.join(".bashrc");
        assert!(find_units_for_path(&bashrc, &units).is_empty());
    }

    #[test]
    fn test_find_units_for_path_overlap() {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let units = vec![
            PathBuf::from(".config"),
            PathBuf::from(".config/nvim"),
            PathBuf::from(".config/nvim/lua"),
        ];

        let nvim_init = home.join(".config/nvim/init.lua");
        let matches = find_units_for_path(&nvim_init, &units);
        assert_eq!(
            matches,
            vec![PathBuf::from(".config/nvim"), PathBuf::from(".config")]
        );

        let lua_file = home.join(".config/nvim/lua/plugins.lua");
        let matches = find_units_for_path(&lua_file, &units);
        assert_eq!(
            matches,
            vec![
                PathBuf::from(".config/nvim/lua"),
                PathBuf::from(".config/nvim"),
                PathBuf::from(".config")
            ]
        );
    }
}
