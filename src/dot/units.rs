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

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::localrepo::LocalRepo;
use crate::dot::repo::RepositoryManager;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Find the unit path that a target path belongs to (if any).
///
/// Returns the unit path if the file is within a unit directory, None otherwise.
pub fn find_unit_for_path(target_path: &Path, units: &[PathBuf]) -> Option<PathBuf> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    for unit in units {
        // Unit paths are relative to home, e.g. ".config/nvim"
        let unit_full_path = home.join(unit);
        if target_path.starts_with(&unit_full_path) {
            return Some(unit.clone());
        }
    }
    None
}

/// Get all effective units by combining global config units and repo-defined units.
///
/// Returns a deduplicated list of unit paths (relative to home directory).
pub fn get_all_units(config: &Config, db: &Database) -> Result<Vec<PathBuf>> {
    let mut units_set: HashSet<PathBuf> = HashSet::new();

    // Add global config units
    for unit in &config.units {
        let path = if unit.starts_with('~') {
            PathBuf::from(shellexpand::tilde(unit).to_string())
                .strip_prefix(shellexpand::tilde("~").to_string())
                .unwrap_or(Path::new(unit))
                .to_path_buf()
        } else {
            PathBuf::from(unit)
        };
        units_set.insert(path);
    }

    // Add repo-defined units
    let repo_manager = RepositoryManager::new(config, db);
    for repo_config in &config.repos {
        if !repo_config.enabled {
            continue;
        }
        if let Ok(local_repo) = repo_manager.get_repository_info(&repo_config.name) {
            for unit in &local_repo.meta.units {
                units_set.insert(PathBuf::from(unit));
            }
        }
    }

    Ok(units_set.into_iter().collect())
}

/// Check if any file in the same unit as the dotfile is modified.
///
/// Returns `(is_any_sibling_modified, unit_path)` where:
/// - `is_any_sibling_modified` is true if any tracked file in the same unit has been modified
/// - `unit_path` is Some if the file belongs to a unit, None otherwise
pub fn is_unit_modified(
    dotfile: &Dotfile,
    all_dotfiles: &HashMap<PathBuf, Dotfile>,
    units: &[PathBuf],
    db: &Database,
) -> Result<(bool, Option<PathBuf>)> {
    // Check if this dotfile belongs to a unit
    let unit_path = match find_unit_for_path(&dotfile.target_path, units) {
        Some(path) => path,
        None => return Ok((false, None)),
    };

    // Check all siblings in the same unit
    for sibling in get_unit_siblings(dotfile, all_dotfiles, units) {
        if !sibling.is_target_unmodified(db)? {
            return Ok((true, Some(unit_path)));
        }
    }

    Ok((false, Some(unit_path)))
}

/// Get all dotfiles that belong to the same unit as the given dotfile.
///
/// Returns an empty vec if the dotfile doesn't belong to any unit.
pub fn get_unit_siblings<'a>(
    dotfile: &Dotfile,
    all_dotfiles: &'a HashMap<PathBuf, Dotfile>,
    units: &[PathBuf],
) -> Vec<&'a Dotfile> {
    let unit_path = match find_unit_for_path(&dotfile.target_path, units) {
        Some(path) => path,
        None => return vec![],
    };

    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let unit_full_path = home.join(&unit_path);

    all_dotfiles
        .values()
        .filter(|df| df.target_path.starts_with(&unit_full_path))
        .collect()
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
    let mut modified_units = HashSet::new();

    for dotfile in all_dotfiles.values() {
        if let Some(unit_path) = find_unit_for_path(&dotfile.target_path, units) {
            // Skip if we already know this unit is modified
            if modified_units.contains(&unit_path) {
                continue;
            }

            // Check if this file is modified
            if !dotfile.is_target_unmodified(db)? {
                modified_units.insert(unit_path);
            }
        }
    }

    Ok(modified_units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot::db::Database;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_find_unit_for_path() {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let units = vec![
            PathBuf::from(".config/nvim"),
            PathBuf::from(".config/helix"),
        ];

        // File inside a unit
        let nvim_init = home.join(".config/nvim/init.lua");
        assert_eq!(
            find_unit_for_path(&nvim_init, &units),
            Some(PathBuf::from(".config/nvim"))
        );

        // File in nested unit path
        let nvim_lua = home.join(".config/nvim/lua/plugins.lua");
        assert_eq!(
            find_unit_for_path(&nvim_lua, &units),
            Some(PathBuf::from(".config/nvim"))
        );

        // File not in any unit
        let bashrc = home.join(".bashrc");
        assert_eq!(find_unit_for_path(&bashrc, &units), None);

        // File in different directory
        let other = home.join(".config/kitty/kitty.conf");
        assert_eq!(find_unit_for_path(&other, &units), None);
    }

    #[test]
    fn test_get_unit_siblings() {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let units = vec![PathBuf::from(".config/nvim")];

        let mut dotfiles = HashMap::new();

        let nvim_init = Dotfile {
            source_path: PathBuf::from("/repo/dots/.config/nvim/init.lua"),
            target_path: home.join(".config/nvim/init.lua"),
        };
        let nvim_plugins = Dotfile {
            source_path: PathBuf::from("/repo/dots/.config/nvim/lua/plugins.lua"),
            target_path: home.join(".config/nvim/lua/plugins.lua"),
        };
        let bashrc = Dotfile {
            source_path: PathBuf::from("/repo/dots/.bashrc"),
            target_path: home.join(".bashrc"),
        };

        dotfiles.insert(nvim_init.target_path.clone(), nvim_init.clone());
        dotfiles.insert(nvim_plugins.target_path.clone(), nvim_plugins.clone());
        dotfiles.insert(bashrc.target_path.clone(), bashrc.clone());

        // nvim_init should have nvim_plugins as sibling (and itself)
        let siblings = get_unit_siblings(&nvim_init, &dotfiles, &units);
        assert_eq!(siblings.len(), 2);

        // bashrc should have no siblings (not in a unit)
        let siblings = get_unit_siblings(&bashrc, &dotfiles, &units);
        assert!(siblings.is_empty());
    }
}
