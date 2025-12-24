use crate::dot::config::{self, Config};
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::localrepo::{DotfileDir, LocalRepo};
use crate::dot::types::{DotsDirSelectItem, RepoSelectItem};
use crate::dot::utils::{filter_dotfiles_by_path, get_all_dotfiles, resolve_dotfile_path};
use crate::menu_utils::{FzfResult, FzfWrapper};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Statistics for directory add operation
pub struct DirectoryAddStats {
    pub updated_count: usize,
    pub unchanged_count: usize,
    pub added_count: usize,
    pub modified_repos: HashSet<PathBuf>,
}

impl DirectoryAddStats {
    pub fn new() -> Self {
        Self {
            updated_count: 0,
            unchanged_count: 0,
            added_count: 0,
            modified_repos: HashSet::new(),
        }
    }

    pub fn has_changes(&self) -> bool {
        self.updated_count > 0 || self.unchanged_count > 0 || self.added_count > 0
    }
}

/// Prompt the user to select one of the configured repositories
fn select_repo(config: &Config) -> Result<config::Repo> {
    if config.repos.is_empty() {
        return Err(anyhow::anyhow!("No repositories configured"));
    }

    if config.repos.len() == 1 {
        let repo = &config.repos[0];
        if !repo.read_only {
            return Ok(repo.clone());
        }
        // If the only repo is read-only, fall through to filtering logic which will return error
    }

    let items: Vec<RepoSelectItem> = config
        .repos
        .iter()
        .filter(|r| {
            if r.read_only {
                println!(
                    "{} Skipping read-only repository '{}'",
                    char::from(NerdFont::ArrowRight).to_string().blue(),
                    r.name
                );
                false
            } else {
                true
            }
        })
        .cloned()
        .map(|repo| RepoSelectItem { repo })
        .collect();

    if items.is_empty() {
        return Err(anyhow::anyhow!("No writable repositories configured"));
    }

    match FzfWrapper::builder()
        .prompt("Select repository to add the dotfile to: ")
        .select(items)
        .map_err(|e| anyhow::anyhow!("Selection error: {}", e))?
    {
        FzfResult::Selected(item) => Ok(item.repo),
        FzfResult::Cancelled => Err(anyhow::anyhow!("No repository selected")),
        FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => Err(anyhow::anyhow!("Unexpected selection result")),
    }
}

/// Prompt the user to select one of the repo's configured `dots_dirs`
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
        FzfResult::Selected(item) => Ok(item.dots_dir),
        FzfResult::Cancelled => Err(anyhow::anyhow!("No dots directory selected")),
        FzfResult::Error(e) => Err(anyhow::anyhow!("Selection error: {}", e)),
        _ => Err(anyhow::anyhow!("Unexpected selection result")),
    }
}

/// Add dotfiles to tracking or update existing tracked files
///
/// Behavior:
/// - For tracked files: Update the source file (fetch behavior)
/// - For untracked files: Prompt to add them to a repository
/// - For directories without --all: Only update tracked files
/// - For directories with --all: Update tracked files AND add untracked files
/// - With --choose: Pick which repo/subdir to add the file to
pub fn add_dotfile(
    config: &Config,
    db: &Database,
    path: &str,
    add_all: bool,
    choose: bool,
    debug: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let target_path = resolve_dotfile_path(path)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    // Handle --choose flag for single files
    if choose && target_path.is_file() {
        return add_with_destination_picker(config, db, &target_path);
    }

    // Get tracked dotfiles within the specified path
    let tracked_dotfiles = filter_dotfiles_by_path(&all_dotfiles, &target_path);

    let mut stats = DirectoryAddStats::new();

    // Update tracked files
    if !tracked_dotfiles.is_empty() {
        update_tracked_dotfiles(&tracked_dotfiles, config, db, &mut stats, debug)?;
    }

    // Handle untracked files
    if add_all {
        // Scan for untracked files and add them
        let (_, untracked_files) = scan_and_categorize_files(&target_path, &all_dotfiles);
        add_untracked_files(&untracked_files, config, db, &mut stats, debug)?;
    } else if target_path.is_file() && tracked_dotfiles.is_empty() {
        // Single untracked file - prompt to add it
        let repo_path = add_new_file(config, db, &target_path)?;
        stats.added_count += 1;
        stats.modified_repos.insert(repo_path);
    } else if tracked_dotfiles.is_empty() {
        // Directory with no tracked files
        let relative_dir = target_path.strip_prefix(&home).unwrap_or(&target_path);
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

    db.cleanup_hashes(config.hash_cleanup_days)?;
    print_directory_add_summary(&stats);

    Ok(())
}

/// Add a file with a destination picker (for --choose flag)
fn add_with_destination_picker(
    config: &Config,
    db: &Database,
    target_path: &PathBuf,
) -> Result<()> {
    use super::alternative::{add_to_destination, get_all_destinations};
    use crate::dot::override_config::{DotfileSource, find_all_sources};

    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let display_path = target_path
        .strip_prefix(&home)
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| target_path.display().to_string());

    // Find existing sources for this file
    let existing_sources = find_all_sources(config, target_path)?;
    let all_destinations = get_all_destinations(config)?;

    if all_destinations.is_empty() {
        emit(
            Level::Warn,
            "dot.add.no_repos",
            &format!(
                "{} No writable repositories configured",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(());
    }

    // Build selection items, marking which ones already have the file
    #[derive(Clone)]
    struct DestItem {
        source: DotfileSource,
        exists: bool,
    }

    impl crate::menu_utils::FzfSelectable for DestItem {
        fn fzf_display_text(&self) -> String {
            let status = if self.exists { " [exists]" } else { "" };
            format!(
                "{} / {}{}",
                self.source.repo_name, self.source.subdir_name, status
            )
        }
        fn fzf_key(&self) -> String {
            format!("{}:{}", self.source.repo_name, self.source.subdir_name)
        }
    }

    let items: Vec<DestItem> = all_destinations
        .into_iter()
        .map(|dest| {
            let exists = existing_sources
                .iter()
                .any(|s| s.repo_name == dest.repo_name && s.subdir_name == dest.subdir_name);
            DestItem {
                source: dest,
                exists,
            }
        })
        .collect();

    let prompt = format!("Select destination for {}: ", display_path);
    match FzfWrapper::builder().prompt(prompt).select(items)? {
        FzfResult::Selected(item) => {
            if item.exists {
                emit(
                    Level::Info,
                    "dot.add.already_exists",
                    &format!(
                        "{} {} already exists in {} / {}",
                        char::from(NerdFont::Info),
                        display_path.cyan(),
                        item.source.repo_name.green(),
                        item.source.subdir_name.green()
                    ),
                    None,
                );
            } else {
                add_to_destination(config, db, target_path, &item.source)?;
            }
        }
        FzfResult::Cancelled => {
            emit(
                Level::Info,
                "dot.add.cancelled",
                &format!("{} Selection cancelled", char::from(NerdFont::Info)),
                None,
            );
        }
        FzfResult::Error(e) => {
            return Err(anyhow::anyhow!("Selection error: {}", e));
        }
        _ => {}
    }

    Ok(())
}

/// Add a new untracked file and return the repo path
fn add_new_file(config: &Config, db: &Database, full_path: &Path) -> Result<PathBuf> {
    use super::alternative::add_to_destination;
    use crate::dot::override_config::DotfileSource;

    // Repository selection
    let repo_config = select_repo(config)?;
    let local_repo = LocalRepo::new(config, repo_config.name.clone())?;

    // dots_dir selection
    let chosen_dir = select_dots_dir(&local_repo)?;

    // Build destination info
    let repo_base = local_repo.local_path(config)?;
    let dest = DotfileSource {
        repo_name: repo_config.name.clone(),
        subdir_name: chosen_dir
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string()),
        source_path: chosen_dir.path.clone(),
    };

    // Use shared add function
    add_to_destination(config, db, &full_path.to_path_buf(), &dest)?;

    Ok(repo_base)
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

/// Update a single tracked dotfile and return whether it was updated or unchanged
fn update_single_dotfile(dotfile: &Dotfile, db: &Database) -> Result<bool> {
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
    config: &Config,
    db: &Database,
    stats: &mut DirectoryAddStats,
    debug: bool,
) -> Result<()> {
    for dotfile in dotfiles {
        // Check if repo is read-only
        let repo_name = crate::dot::git::get_repo_name_for_dotfile(dotfile, config);
        if let Some(repo) = config.repos.iter().find(|r| r.name == repo_name.as_str())
            && repo.read_only
        {
            println!(
                "{} Skipping update for read-only repository '{}'",
                char::from(NerdFont::ArrowRight).to_string().blue(),
                repo.name
            );
            continue;
        }

        let was_updated = update_single_dotfile(dotfile, db)?;
        if was_updated {
            stats.updated_count += 1;
            let local_repo = LocalRepo::new(config, repo_name.to_string())?;
            let repo_path = local_repo.local_path(config)?;

            // Automatically stage the updated file
            // resolve_dotfile_path might be needed if source_path is not absolute, but Dotfile struct seems to keep absolute paths.
            // checking dotfile.rs... source_path should be absolute.
            if let Err(e) =
                crate::dot::git::repo_ops::git_add(&repo_path, &dotfile.source_path, debug)
            {
                eprintln!(
                    "{} Failed to stage file: {}",
                    char::from(NerdFont::Warning).to_string().yellow(),
                    e
                );
            }

            stats.modified_repos.insert(repo_path);
        } else {
            stats.unchanged_count += 1;
        }
    }
    Ok(())
}

/// Add multiple untracked files
fn add_untracked_files(
    file_paths: &[PathBuf],
    config: &Config,
    db: &Database,
    stats: &mut DirectoryAddStats,
    debug: bool,
) -> Result<()> {
    if file_paths.is_empty() {
        return Ok(());
    }

    println!(
        "\n{} Adding {} untracked file(s)...",
        char::from(NerdFont::Info),
        file_paths.len()
    );

    for file_path in file_paths {
        let repo_path = add_new_file(config, db, file_path)?;
        stats.added_count += 1;
        stats.modified_repos.insert(repo_path);
    }

    Ok(())
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

        if !stats.modified_repos.is_empty() {
            let mut repos: Vec<_> = stats.modified_repos.iter().collect();
            repos.sort();

            println!(
                "{} Modified repositories:",
                char::from(NerdFont::Info).to_string().blue()
            );
            for repo_path in repos {
                println!("  {}", repo_path.display().to_string().cyan());
            }
        }
    } else {
        emit(
            Level::Info,
            "dot.add.no_changes",
            &format!("{} No changes to process", char::from(NerdFont::Info)),
            None,
        );
    }
}
