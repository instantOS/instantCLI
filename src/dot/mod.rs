use crate::fzf_wrapper::{FzfOptions, FzfSelectable, FzfWrapper};
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

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
        crate::fzf_wrapper::FzfPreview::Text(format!(
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

    fn fzf_preview(&self) -> crate::fzf_wrapper::FzfPreview {
        crate::fzf_wrapper::FzfPreview::Text(format!(
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
pub use git::{status_all, update_all};

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
        // For relative paths, resolve relative to current working directory
        let current_dir = std::env::current_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
        current_dir.join(path)
    };

    // Canonicalize the path to resolve any symlinks or relative components
    let canonical_path = resolved_path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Failed to resolve path '{}': {}", path, e))?;

    // Validate that the path is within the home directory
    if !canonical_path.starts_with(&home) {
        return Err(anyhow::anyhow!(
            "Path '{}' is outside the home directory. Only files in {} are allowed.",
            canonical_path.display(),
            home.display()
        ));
    }

    Ok(canonical_path)
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

pub fn fetch_modified(
    config: &Config,
    db: &Database,
    path: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let modified_dotfiles = get_modified_dotfiles(config, db, path)?;

    if modified_dotfiles.is_empty() {
        println!("{}", "No modified dotfiles to fetch.".green());
        return Ok(());
    }

    let grouped_by_repo = group_dotfiles_by_repo(&modified_dotfiles, config)?;

    print_fetch_plan(&grouped_by_repo, dry_run)?;

    if !dry_run {
        fetch_dotfiles(&modified_dotfiles, db, config.hash_cleanup_days)?;
    }

    Ok(())
}

fn get_modified_dotfiles(
    config: &Config,
    db: &Database,
    path: Option<&str>,
) -> Result<Vec<Dotfile>> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let mut modified_dotfiles = Vec::new();

    if let Some(p) = path {
        let full_path = resolve_dotfile_path(p)?;

        for (target_path, dotfile) in all_dotfiles {
            if target_path.starts_with(&full_path) && !dotfile.is_target_unmodified(db)? {
                modified_dotfiles.push(dotfile);
            }
        }
    } else {
        for (_, dotfile) in all_dotfiles {
            if !dotfile.is_target_unmodified(db)? {
                modified_dotfiles.push(dotfile);
            }
        }
    }

    Ok(modified_dotfiles)
}

/// Helper function to find which repository contains a dotfile
fn find_repo_for_dotfile(dotfile: &Dotfile, config: &Config) -> Result<Option<RepoName>> {
    for repo in &config.repos {
        let local_repo = LocalRepo::new(config, repo.name.clone())?;
        if dotfile
            .source_path
            .starts_with(local_repo.local_path(config)?)
        {
            return Ok(Some(RepoName::new(repo.name.clone())));
        }
    }
    Ok(None)
}

fn group_dotfiles_by_repo<'a>(
    dotfiles: &'a [Dotfile],
    config: &Config,
) -> Result<HashMap<RepoName, Vec<&'a Dotfile>>> {
    let mut grouped_by_repo: HashMap<RepoName, Vec<&Dotfile>> = HashMap::new();

    for dotfile in dotfiles {
        if let Some(repo_name) = find_repo_for_dotfile(dotfile, config)? {
            grouped_by_repo.entry(repo_name).or_default().push(dotfile);
        }
    }

    Ok(grouped_by_repo)
}

fn print_fetch_plan(
    grouped_by_repo: &HashMap<RepoName, Vec<&Dotfile>>,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        println!(
            "{}",
            "Dry run: The following files would be fetched:".yellow()
        );
    } else {
        println!("{}", "Fetching the following modified files:".yellow());
    }

    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    for (repo_name, dotfiles) in grouped_by_repo {
        println!("  Repo: {}", repo_name.as_str().bold());
        for dotfile in dotfiles {
            let relative_path = dotfile.target_path.strip_prefix(&home).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to strip prefix from path {}: {}",
                    dotfile.target_path.display(),
                    e
                )
            })?;
            println!("    - ~/{}", relative_path.display());
        }
    }
    Ok(())
}

fn fetch_dotfiles(dotfiles: &[Dotfile], db: &Database, hash_cleanup_days: u32) -> Result<()> {
    for dotfile in dotfiles {
        dotfile.fetch(db)?;
    }
    db.cleanup_hashes(hash_cleanup_days)?;
    println!("\n{}", "Fetch complete.".green());
    Ok(())
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
            println!("Created new dotfile: ~/{relative}");
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

    let wrapper = FzfWrapper::with_options(FzfOptions {
        prompt: Some("Select repository to add the dotfile to: ".to_string()),
        preview_window: Some("right:40%".to_string()),
        ..Default::default()
    });

    match wrapper
        .select(items)
        .map_err(|e| anyhow::anyhow!("Selection error: {}", e))?
    {
        crate::fzf_wrapper::FzfResult::Selected(item) => Ok(item.repo),
        crate::fzf_wrapper::FzfResult::Cancelled => Err(anyhow::anyhow!("No repository selected")),
        crate::fzf_wrapper::FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
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

    let wrapper = FzfWrapper::with_options(FzfOptions {
        prompt: Some(format!(
            "Select target dots_dir in repo '{}': ",
            local_repo.name
        )),
        preview_window: Some("right:40%".to_string()),
        ..Default::default()
    });

    match wrapper
        .select(items)
        .map_err(|e| anyhow::anyhow!("Selection error: {}", e))?
    {
        crate::fzf_wrapper::FzfResult::Selected(item) => Ok(item.dots_dir),
        crate::fzf_wrapper::FzfResult::Cancelled => {
            Err(anyhow::anyhow!("No dots directory selected"))
        }
        crate::fzf_wrapper::FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => Err(anyhow::anyhow!("Unexpected selection result")),
    }
}

/// Add a new dotfile to tracking
pub fn add_dotfile(config: &Config, db: &Database, path: &str) -> Result<()> {
    // Resolve the path using git-style resolution
    let full_path = resolve_dotfile_path(path)?;

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
    let relative = full_path.strip_prefix(&home).unwrap_or(&full_path);
    let dest_path = dest_base.join(relative);

    // Use Dotfile methods to perform the copy and DB registration
    let dotfile = Dotfile {
        source_path: dest_path.clone(),
        target_path: full_path.clone(),
    };
    // If the source already exists, treat as overwrite; Dotfile methods may be extended
    // later to prompt or handle conflicts more gracefully.
    dotfile.create_source_from_target(db)?;

    let chosen_dir_name = chosen_dir
        .path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| chosen_dir.path.display().to_string());

    println!(
        "Added {} to repo '{}' in directory '{}'",
        path, local_repo.name, chosen_dir_name
    );

    Ok(())
}
