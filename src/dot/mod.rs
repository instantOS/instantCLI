use crate::menu_utils::{FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoName(String);

/// Helper struct for repository selection
#[derive(Debug, Clone)]
pub struct RepoSelectItem {
    pub repo: config::Repo,
}

impl FzfSelectable for RepoSelectItem {
    fn fzf_display_text(&self) -> String {
        self.repo.name.clone()
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(format!(
            "URL: {}\nBranch: {}\nEnabled: {}",
            self.repo.url,
            self.repo.branch.as_deref().unwrap_or("default"),
            if self.repo.enabled { "Yes" } else { "No" }
        ))
    }
}

/// Helper struct for dots directory selection
#[derive(Debug, Clone)]
pub struct DotsDirSelectItem {
    pub dots_dir: DotfileDir,
    pub repo_name: String,
}

impl FzfSelectable for DotsDirSelectItem {
    fn fzf_display_text(&self) -> String {
        self.dots_dir
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.dots_dir.path.display().to_string())
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(format!(
            "Repository: {}\nPath: {}\nActive: {}",
            self.repo_name,
            self.dots_dir.path.display(),
            if self.dots_dir.is_active { "Yes" } else { "No" }
        ))
    }
}

impl RepoName {
    pub fn new(name: String) -> Self {
        RepoName(name)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for RepoName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for RepoName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

use walkdir::WalkDir;

pub mod config;
pub mod db;
pub mod dotfile;
pub mod git;
pub mod localrepo;
pub mod meta;
pub mod path_serde;
pub mod repo;

#[cfg(test)]
mod path_tests;

pub use crate::dot::dotfile::Dotfile;
pub use git::{diff_all, status_all, update_all};

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::{DotfileDir, LocalRepo};

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
    use crate::dot::repo::RepositoryManager;

    let repo_manager = RepositoryManager::new(config, db);
    repo_manager.get_active_dotfile_dirs()
}

/// Helper function to scan a directory for dotfiles
// should only be run within a dotfile subdir, NOT the home directory
fn scan_directory_for_dotfiles(dir_path: &Path, home_path: &Path) -> Result<Vec<Dotfile>> {
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
fn merge_dotfiles(dotfiles_list: Vec<Vec<Dotfile>>) -> HashMap<PathBuf, Dotfile> {
    let mut filemap = HashMap::new();

    // Process in order - later repos override earlier ones
    for dotfiles in dotfiles_list {
        for dotfile in dotfiles {
            filemap.insert(dotfile.target_path.clone(), dotfile);
        }
    }

    filemap
}

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


pub fn apply_all(config: &Config, db: &Database) -> Result<()> {
    let filemap = get_all_dotfiles(config, db)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    for dotfile in filemap.values() {
        let was_missing = !dotfile.target_path.exists();
        dotfile.apply(db)?;
        if was_missing {
            let relative = dotfile
                .target_path
                .strip_prefix(&home)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to strip prefix from path {}: {}",
                        dotfile.target_path.display(),
                        e
                    )
                })?
                .to_string_lossy();
            emit(
                Level::Success,
                "dot.apply.created",
                &format!(
                    "{} Created new dotfile: ~/{relative}",
                    char::from(NerdFont::Check)
                ),
                None,
            );
        }
    }
    db.cleanup_hashes(config.hash_cleanup_days)?;
    Ok(())
}

pub fn reset_modified(config: &Config, db: &Database, path: &str) -> Result<()> {
    let filemap = get_all_dotfiles(config, db)?;
    let full_path = resolve_dotfile_path(path)?;

    let mut reset_files = Vec::new();
    let mut already_clean_files = Vec::new();

    for dotfile in filemap.values() {
        if dotfile.target_path.starts_with(&full_path) {
            if !dotfile.is_target_unmodified(db)? {
                dotfile.reset(db)?;
                reset_files.push(dotfile.target_path.clone());
            } else {
                already_clean_files.push(dotfile.target_path.clone());
            }
        }
    }

    // Print results
    if !reset_files.is_empty() {
        println!("{}", "Reset the following modified files:".green());
        for file_path in &reset_files {
            println!("  {}", file_path.display());
        }
    } else if !already_clean_files.is_empty() {
        println!(
            "{}",
            "No files needed reset - all files are already clean".green()
        );
    } else {
        println!("{}", "No dotfiles found in the specified path".yellow());
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;
    Ok(())
}

/// Prompt the user to select one of the configured repositories.
fn select_repo(config: &Config) -> Result<config::Repo> {
    if config.repos.is_empty() {
        return Err(anyhow::anyhow!("No repositories configured"));
    }

    if config.repos.len() == 1 {
        return Ok(config.repos[0].clone());
    }

    let items: Vec<RepoSelectItem> = config
        .repos
        .iter()
        .cloned()
        .map(|repo| RepoSelectItem { repo })
        .collect();

    match FzfWrapper::builder()
        .prompt("Select repository to add the dotfile to: ")
        .select(items)
        .map_err(|e| anyhow::anyhow!("Selection error: {}", e))?
    {
        crate::menu_utils::FzfResult::Selected(item) => Ok(item.repo),
        crate::menu_utils::FzfResult::Cancelled => Err(anyhow::anyhow!("No repository selected")),
        crate::menu_utils::FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => Err(anyhow::anyhow!("Unexpected selection result")),
    }
}

/// Prompt the user to select one of the repo's configured `dots_dirs`.
fn select_dots_dir(local_repo: &LocalRepo) -> Result<DotfileDir> {
    let dirs = &local_repo.dotfile_dirs;

    if dirs.is_empty() {
        return Err(anyhow::anyhow!(
            "Repository '{}' has no configured dots_dirs",
            local_repo.name
        ));
    }

    if dirs.len() == 1 {
        return Ok(dirs[0].clone());
    }

    let items: Vec<DotsDirSelectItem> = dirs
        .iter()
        .cloned()
        .map(|dots_dir| DotsDirSelectItem {
            dots_dir,
            repo_name: local_repo.name.clone(),
        })
        .collect();

    match FzfWrapper::builder()
        .prompt(format!(
            "Select target dots_dir in repo '{}': ",
            local_repo.name
        ))
        .select(items)
        .map_err(|e| anyhow::anyhow!("Selection error: {}", e))?
    {
        crate::menu_utils::FzfResult::Selected(item) => Ok(item.dots_dir),
        crate::menu_utils::FzfResult::Cancelled => {
            Err(anyhow::anyhow!("No dots directory selected"))
        }
        crate::menu_utils::FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => Err(anyhow::anyhow!("Unexpected selection result")),
    }
}

/// Add dotfiles to tracking or update existing tracked files
///
/// Behavior:
/// - For a single file: If tracked, update the source file. If untracked, prompt to add it.
/// - For a directory without --all: Update all tracked files, skip untracked files with info message.
/// - For a directory with --all: Recursively add all files, including untracked ones.
pub fn add_dotfile(config: &Config, db: &Database, path: &str, add_all: bool) -> Result<()> {
    let full_path = resolve_dotfile_path(path)?;

    if full_path.is_file() {
        add_single_file(config, db, &full_path)?;
    } else if full_path.is_dir() {
        add_directory(config, db, &full_path, add_all)?;
    } else {
        return Err(anyhow::anyhow!(
            "Path '{}' is neither a file nor a directory",
            path
        ));
    }

    Ok(())
}

/// Add or update a single file
fn add_single_file(config: &Config, db: &Database, full_path: &Path) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;

    // Check if this file is already tracked
    if let Some(dotfile) = all_dotfiles.get(full_path) {
        // File is tracked - update it (fetch behavior)
        update_tracked_file(dotfile, db)?;
    } else {
        // File is not tracked - add it (old add behavior)
        add_new_file(config, db, full_path)?;
    }

    Ok(())
}

/// Update a tracked file by copying from target to source
fn update_tracked_file(dotfile: &Dotfile, db: &Database) -> Result<()> {
    let was_modified = !dotfile.is_target_unmodified(db)?;

    if was_modified {
        // Compute hashes before and after to detect changes
        let old_source_hash = if dotfile.source_path.exists() {
            Some(Dotfile::compute_hash(&dotfile.source_path)?)
        } else {
            None
        };

        dotfile.fetch(db)?;

        let new_source_hash = Dotfile::compute_hash(&dotfile.source_path)?;

        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let relative_path = dotfile
            .target_path
            .strip_prefix(&home)
            .unwrap_or(&dotfile.target_path);

        if old_source_hash.as_ref() != Some(&new_source_hash) {
            println!(
                "{} Updated ~/{} (changes detected)",
                char::from(NerdFont::Check),
                relative_path.display().to_string().green()
            );
        } else {
            println!(
                "{} ~/{} (no changes)",
                char::from(NerdFont::Info),
                relative_path.display().to_string().dimmed()
            );
        }
    } else {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        let relative_path = dotfile
            .target_path
            .strip_prefix(&home)
            .unwrap_or(&dotfile.target_path);
        println!(
            "{} ~/{} (already in sync)",
            char::from(NerdFont::Info),
            relative_path.display().to_string().dimmed()
        );
    }

    Ok(())
}

/// Add a new untracked file
fn add_new_file(config: &Config, db: &Database, full_path: &Path) -> Result<()> {
    // Repository selection
    let repo_config = select_repo(config)?;
    let local_repo = LocalRepo::new(config, repo_config.name.clone())?;

    // dots_dir selection
    let chosen_dir = select_dots_dir(&local_repo)?;

    // Construct destination path inside the repo
    let repo_base = local_repo.local_path(config)?;
    let dest_base = repo_base.join(&chosen_dir.path);

    // Compute relative path from home and final destination
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative = full_path.strip_prefix(&home).unwrap_or(full_path);
    let dest_path = dest_base.join(relative);

    // Use Dotfile methods to perform the copy and DB registration
    let dotfile = Dotfile {
        source_path: dest_path.clone(),
        target_path: full_path.to_path_buf(),
    };

    dotfile.create_source_from_target(db)?;

    let chosen_dir_name = chosen_dir
        .path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| chosen_dir.path.display().to_string());

    let relative_display = relative.display().to_string();
    println!(
        "{} Added ~/{} to repo '{}' in directory '{}'",
        char::from(NerdFont::Check),
        relative_display.green(),
        local_repo.name,
        chosen_dir_name
    );

    Ok(())
}

/// Statistics for directory add operation
struct DirectoryAddStats {
    updated_count: usize,
    unchanged_count: usize,
    added_count: usize,
}

impl DirectoryAddStats {
    fn new() -> Self {
        Self {
            updated_count: 0,
            unchanged_count: 0,
            added_count: 0,
        }
    }

    fn has_changes(&self) -> bool {
        self.updated_count > 0 || self.unchanged_count > 0 || self.added_count > 0
    }
}

/// Scan directory and categorize files as tracked or untracked
fn scan_and_categorize_files(
    dir_path: &Path,
    all_dotfiles: &HashMap<PathBuf, Dotfile>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut tracked_files = Vec::new();
    let mut untracked_files = Vec::new();

    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            let path_str = entry.path().to_string_lossy();
            !path_str.contains("/.git/")
        })
    {
        if entry.file_type().is_file() {
            let file_path = entry.path();

            if all_dotfiles.contains_key(file_path) {
                tracked_files.push(file_path.to_path_buf());
            } else {
                untracked_files.push(file_path.to_path_buf());
            }
        }
    }

    (tracked_files, untracked_files)
}

/// Get tracked dotfiles within a directory (without filesystem traversal)
fn get_tracked_files_in_dir<'a>(
    dir_path: &Path,
    all_dotfiles: &'a HashMap<PathBuf, Dotfile>,
) -> Vec<&'a Dotfile> {
    all_dotfiles
        .values()
        .filter(|dotfile| dotfile.target_path.starts_with(dir_path))
        .collect()
}

/// Update a single tracked dotfile and return whether it was updated or unchanged
fn update_single_dotfile(dotfile: &Dotfile, db: &Database) -> Result<bool> {
    let was_modified = !dotfile.is_target_unmodified(db)?;

    if !was_modified {
        return Ok(false); // unchanged
    }

    let old_source_hash = if dotfile.source_path.exists() {
        Some(Dotfile::compute_hash(&dotfile.source_path)?)
    } else {
        None
    };

    dotfile.fetch(db)?;

    let new_source_hash = Dotfile::compute_hash(&dotfile.source_path)?;
    let has_changes = old_source_hash.as_ref() != Some(&new_source_hash);

    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let relative_path = dotfile
        .target_path
        .strip_prefix(&home)
        .unwrap_or(&dotfile.target_path);

    if has_changes {
        println!(
            "{} Updated ~/{} (changes detected)",
            char::from(NerdFont::Check),
            relative_path.display().to_string().green()
        );
    }

    Ok(has_changes)
}

/// Update multiple tracked dotfiles
fn update_tracked_dotfiles(
    dotfiles: &[&Dotfile],
    db: &Database,
    stats: &mut DirectoryAddStats,
) -> Result<()> {
    for dotfile in dotfiles {
        let was_updated = update_single_dotfile(dotfile, db)?;
        if was_updated {
            stats.updated_count += 1;
        } else {
            stats.unchanged_count += 1;
        }
    }
    Ok(())
}

/// Update tracked dotfiles from a list of file paths
fn update_tracked_files_by_path(
    file_paths: &[PathBuf],
    all_dotfiles: &HashMap<PathBuf, Dotfile>,
    db: &Database,
    stats: &mut DirectoryAddStats,
) -> Result<()> {
    for file_path in file_paths {
        if let Some(dotfile) = all_dotfiles.get(file_path) {
            let was_updated = update_single_dotfile(dotfile, db)?;
            if was_updated {
                stats.updated_count += 1;
            } else {
                stats.unchanged_count += 1;
            }
        }
    }
    Ok(())
}

/// Add multiple untracked files
fn add_untracked_files(
    file_paths: &[PathBuf],
    config: &Config,
    db: &Database,
) -> Result<usize> {
    if file_paths.is_empty() {
        return Ok(0);
    }

    println!(
        "\n{} Adding {} untracked file(s)...",
        char::from(NerdFont::Info),
        file_paths.len()
    );

    for file_path in file_paths {
        add_new_file(config, db, file_path)?;
    }

    Ok(file_paths.len())
}

/// Print summary of directory add operation
fn print_directory_add_summary(stats: &DirectoryAddStats) {
    if stats.has_changes() {
        emit(
            Level::Success,
            "dot.add.complete",
            &format!(
                "{} Complete: {} updated, {} unchanged{}",
                char::from(NerdFont::Check),
                stats.updated_count,
                stats.unchanged_count,
                if stats.added_count > 0 {
                    format!(", {} added", stats.added_count)
                } else {
                    String::new()
                }
            ),
            None,
        );
    } else {
        emit(
            Level::Info,
            "dot.add.no_changes",
            &format!("{} No changes to process", char::from(NerdFont::Info)),
            None,
        );
    }
}

/// Add or update files in a directory
fn add_directory(config: &Config, db: &Database, dir_path: &Path, add_all: bool) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let mut stats = DirectoryAddStats::new();

    if add_all {
        // With --all: Do full directory traversal to find both tracked and untracked files
        let (tracked_files, untracked_files) = scan_and_categorize_files(dir_path, &all_dotfiles);

        // Update tracked files
        update_tracked_files_by_path(&tracked_files, &all_dotfiles, db, &mut stats)?;

        // Add untracked files
        stats.added_count = add_untracked_files(&untracked_files, config, db)?;
    } else {
        // Without --all: Only process tracked files (no expensive directory traversal)
        let tracked_in_dir = get_tracked_files_in_dir(dir_path, &all_dotfiles);

        if tracked_in_dir.is_empty() {
            let relative_dir = dir_path.strip_prefix(&home).unwrap_or(dir_path);
            emit(
                Level::Info,
                "dot.add.no_tracked",
                &format!(
                    "{} No tracked files found in ~/{}\n  Use 'ins dot add --all ~/{} to add untracked files",
                    char::from(NerdFont::Info),
                    relative_dir.display(),
                    relative_dir.display()
                ),
                None,
            );
            return Ok(());
        }

        // Update tracked files
        update_tracked_dotfiles(&tracked_in_dir, db, &mut stats)?;
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;
    print_directory_add_summary(&stats);

    Ok(())
}
