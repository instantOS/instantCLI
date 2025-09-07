use anyhow::Result;
use shellexpand;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

pub mod config;
pub mod db;
pub mod dotfile;
pub mod git;
pub mod localrepo;
pub mod meta;

pub use crate::dot::dotfile::Dotfile;
pub use git::{add_repo, status_all, update_all};

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::localrepo::LocalRepo;
use std::env::current_dir;
use std::fs;

pub fn get_current_repo(config: &Config, cwd: &Path) -> Result<LocalRepo> {
    let mut this_repo: Option<LocalRepo> = None;
    for repo in &config.repos {
        let local = LocalRepo::from(repo.clone());
        if cwd.starts_with(local.local_path()?) {
            this_repo = Some(local);
            break;
        }
    }
    this_repo.ok_or(anyhow::anyhow!("Not in a dotfile repo"))
}

pub fn get_all_dotfiles() -> Result<HashMap<PathBuf, Dotfile>> {
    let mut filemap = HashMap::new();
    let repos = config::Config::load()?.repos;
    let base_dir = config::repos_base_dir()?;

    for repo in repos {
        let repo_name = repo.name.as_ref().map_or_else(
            || config::basename_from_repo(&repo.url),
            |name| name.clone(),
        );
        let repo_path = base_dir.join(repo_name);
        let dots_path = repo_path.join("dots");
        for entry in WalkDir::new(&dots_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| {
                let path_str = entry.path().to_string_lossy();
                !path_str.contains("/.git/")
            })
        {
            if entry.file_type().is_file() {
                let source_path = entry.path().to_path_buf();
                let relative_path = source_path.strip_prefix(&dots_path).unwrap().to_path_buf();
                let target_path =
                    PathBuf::from(shellexpand::tilde("~").to_string()).join(relative_path);

                let dotfile = Dotfile {
                    repo_path: source_path,
                    target_path: target_path.clone(),
                    hash: None,
                    target_hash: None,
                };

                filemap.insert(target_path, dotfile);
            }
        }
    }

    Ok(filemap)
}

pub fn fetch_modified(path: Option<&str>) -> Result<()> {
    let cwd = current_dir()?;
    let db = Database::new()?;
    let config = Config::load()?;
    let this_repo = get_current_repo(&config, &cwd)?;
    let repo_path = this_repo.local_path()?;
    let dots_path = repo_path.join("dots");
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    if let Some(p) = path {
        let p = p.trim_start_matches('.');
        let home_subdir = home.join(p);
        if !home_subdir.exists() {
            return Ok(());
        }
        let md = fs::metadata(&home_subdir)?;
        if md.is_file() {
            let source_file = dots_path.join(p);
            if !source_file.exists() {
                if let Some(parent) = source_file.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&home_subdir, &source_file)?;
                let dotfile = Dotfile {
                    repo_path: source_file,
                    target_path: home_subdir,
                    hash: None,
                    target_hash: None,
                };
                let _ = dotfile.get_source_hash(&db);
            }
        } else if md.is_dir() {
            let source_subdir = dots_path.join(p);
            if source_subdir.exists() {
                // Walk existing source subdir and fetch tracked
                for entry in WalkDir::new(&source_subdir)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().is_file())
                {
                    let source_file = entry.path().to_path_buf();
                    let relative = source_file.strip_prefix(&dots_path).unwrap().to_path_buf();
                    let target_file = home.join(relative);
                    if target_file.exists() {
                        let dotfile = Dotfile {
                            repo_path: source_file,
                            target_path: target_file,
                            hash: None,
                            target_hash: None,
                        };
                        dotfile.fetch(&db)?;
                    }
                }
            }
            // else do nothing for non-existent source dir
        }
    } else {
        // Global fetch: walk all dots/ and fetch tracked
        for entry in WalkDir::new(&dots_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let source_file = entry.path().to_path_buf();
            let relative = source_file.strip_prefix(&dots_path).unwrap().to_path_buf();
            let target_file = home.join(relative);
            if target_file.exists() {
                let dotfile = Dotfile {
                    repo_path: source_file,
                    target_path: target_file,
                    hash: None,
                    target_hash: None,
                };
                dotfile.fetch(&db)?;
            }
        }
    }
    db.cleanup_hashes()?;
    Ok(())
}

pub fn apply_all() -> Result<()> {
    let db = Database::new()?;
    let filemap = get_all_dotfiles()?;
    for dotfile in filemap.values() {
        dotfile.apply(&db)?;
    }
    db.cleanup_hashes()?;
    Ok(())
}

pub fn reset_modified(path: &str) -> Result<()> {
    let db = Database::new()?;
    let filemap = get_all_dotfiles()?;
    let expanded = shellexpand::tilde(path).into_owned();
    let full_path = PathBuf::from(expanded);
    for dotfile in filemap.values() {
        if dotfile.target_path.starts_with(&full_path) && dotfile.is_modified(&db) {
            dotfile.apply(&db)?;
        }
    }
    db.cleanup_hashes()?;
    Ok(())
}
