use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::repo::RepositoryManager;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Resolve a path argument to an absolute path in the home directory
///
/// This function handles path resolution similar to git:
/// - If path starts with '~', expand it to home directory
/// - If path is absolute, validate it's within home directory
/// - If path is relative, resolve it relative to current working directory,
///   then validate it's within home directory
///
/// Important: This function does NOT canonicalize symlinks because dotfile tracking
/// should work with the user-specified path (which may be a symlink), not the resolved target.
///
/// Returns the resolved absolute path if valid, or an error if:
/// - The path doesn't exist
/// - The path is outside the home directory
pub fn resolve_dotfile_path(path: &str) -> Result<PathBuf> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    let resolved_path = if path.starts_with('~') {
        // Expand tilde to home directory
        PathBuf::from(shellexpand::tilde(path).into_owned())
    } else if Path::new(path).is_absolute() {
        // Use absolute path as-is
        PathBuf::from(path)
    } else {
        // For relative paths, prefer the current working directory and fall back to the home directory
        let current_dir = std::env::current_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
        let candidate = current_dir.join(path);
        if candidate.exists() {
            candidate
        } else {
            home.join(path)
        }
    };

    // Normalize the path by removing redundant components (like ./, ../) but DON'T resolve symlinks
    let normalized_path = normalize_path(&resolved_path)?;

    // Validate that the path exists
    if !normalized_path.exists() {
        return Err(anyhow::anyhow!(
            "Path '{}' does not exist",
            normalized_path.display()
        ));
    }

    // Validate that the path is within the home directory
    // For this check, we need to resolve any symlinks in the parent directories to ensure safety
    let real_path = normalized_path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Failed to validate path '{}': {}", path, e))?;

    if !real_path.starts_with(
        &home
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Failed to canonicalize home directory: {}", e))?,
    ) {
        return Err(anyhow::anyhow!(
            "Path '{}' is outside the home directory. Only files in {} are allowed.",
            normalized_path.display(),
            home.display()
        ));
    }

    Ok(normalized_path)
}

/// Normalize a path by removing redundant components without resolving symlinks
fn normalize_path(path: &Path) -> Result<PathBuf> {
    use std::path::Component;

    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {
                // Skip current directory components
            }
            Component::ParentDir => {
                // Go up one directory if possible
                if !result.pop() {
                    return Err(anyhow::anyhow!(
                        "Path '{}' attempts to go above root",
                        path.display()
                    ));
                }
            }
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                result.push(component);
            }
        }
    }

    Ok(result)
}

/// Get all active dotfile directories from all repositories
pub fn get_active_dotfile_dirs(config: &Config, db: &Database) -> Result<Vec<PathBuf>> {
    let repo_manager = RepositoryManager::new(config, db);
    repo_manager.get_active_dotfile_dirs()
}

/// Helper function to scan a directory for dotfiles
/// Should only be run within a dotfile subdir, NOT the home directory
pub fn scan_directory_for_dotfiles(dir_path: &Path, home_path: &Path) -> Result<Vec<Dotfile>> {
    let mut dotfiles = Vec::new();

    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            let path_str = entry.path().to_string_lossy();
            !path_str.contains("/.git/")
        })
    {
        if entry.file_type().is_file() {
            let source_path = entry.path().to_path_buf();
            let relative_path = source_path
                .strip_prefix(dir_path)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to strip prefix from path {}: {}",
                        source_path.display(),
                        e
                    )
                })?
                .to_path_buf();
            let target_path = home_path.join(relative_path);

            dotfiles.push(Dotfile {
                source_path,
                target_path,
            });
        }
    }

    Ok(dotfiles)
}




/// Helper function to merge dotfiles with later repos overriding earlier ones
pub fn merge_dotfiles(dotfiles_list: Vec<Vec<Dotfile>>) -> HashMap<PathBuf, Dotfile> {
    let mut filemap = HashMap::new();

    // Process in order - later repos override earlier ones
    for dotfiles in dotfiles_list {
        for dotfile in dotfiles {
            filemap.insert(dotfile.target_path.clone(), dotfile);
        }
    }

    filemap
}

/// Get all dotfiles from all active repositories
pub fn get_all_dotfiles(config: &Config, db: &Database) -> Result<HashMap<PathBuf, Dotfile>> {
    let active_dirs = get_active_dotfile_dirs(config, db)?;
    let home_path = PathBuf::from(shellexpand::tilde("~").to_string());

    // Scan each directory for dotfiles
    let mut all_dotfiles = Vec::new();
    for dir_path in active_dirs {
        let dotfiles = scan_directory_for_dotfiles(&dir_path, &home_path)?;
        all_dotfiles.push(dotfiles);
    }

    // Merge with proper override behavior
    Ok(merge_dotfiles(all_dotfiles))
}

/// Filter dotfiles to only those within a specific directory path
///
/// This is a common operation used by many commands (reset, diff, status, add)
/// to work on a subset of tracked dotfiles.
pub fn filter_dotfiles_by_path<'a>(
    all_dotfiles: &'a HashMap<PathBuf, Dotfile>,
    path: &Path,
) -> Vec<&'a Dotfile> {
    all_dotfiles
        .values()
        .filter(|dotfile| dotfile.target_path.starts_with(path))
        .collect()
}

/// Filter dotfiles to only those within a specific directory path, returning owned values
pub fn filter_dotfiles_by_path_owned(
    all_dotfiles: &HashMap<PathBuf, Dotfile>,
    path: &Path,
) -> Vec<Dotfile> {
    all_dotfiles
        .values()
        .filter(|dotfile| dotfile.target_path.starts_with(path))
        .cloned()
        .collect()
}
