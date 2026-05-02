use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::repo::DotfileRepositoryManager;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn resolve_dotfile_path(path: &str, allow_root: bool) -> Result<PathBuf> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    let resolved_path = if path.starts_with('~') {
        PathBuf::from(shellexpand::tilde(path).into_owned())
    } else if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        let current_dir = std::env::current_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
        let candidate = current_dir.join(path);
        if candidate.exists() {
            candidate
        } else {
            home.join(path)
        }
    };

    let normalized_path = normalize_path(&resolved_path)?;

    if !normalized_path.exists() {
        return Err(anyhow::anyhow!(
            "Path '{}' does not exist",
            normalized_path.display()
        ));
    }

    if allow_root {
        if !normalized_path.starts_with(&home) && !normalized_path.is_absolute() {
            return Err(anyhow::anyhow!(
                "Path '{}' is outside allowed directories",
                normalized_path.display()
            ));
        }
        return Ok(normalized_path);
    }

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

fn normalize_path(path: &Path) -> Result<PathBuf> {
    use std::path::Component;

    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
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

pub fn get_active_dotfile_dirs(config: &DotfileConfig, db: &Database) -> Result<Vec<DotfileDir>> {
    let repo_manager = DotfileRepositoryManager::new(config, db);
    repo_manager.get_active_dotfile_dirs()
}

pub fn scan_directory_for_dotfiles(
    dir_path: &Path,
    target_prefix: &Path,
    is_root: bool,
) -> Result<Vec<Dotfile>> {
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
            let target_path = target_prefix.join(relative_path);

            dotfiles.push(Dotfile {
                source_path,
                target_path,
                is_root,
            });
        }
    }

    Ok(dotfiles)
}

pub fn merge_dotfiles(dotfiles_list: Vec<Vec<Dotfile>>) -> HashMap<PathBuf, Dotfile> {
    let mut filemap = HashMap::new();

    for dotfiles in dotfiles_list.into_iter() {
        for dotfile in dotfiles {
            filemap
                .entry(dotfile.target_path.clone())
                .or_insert(dotfile);
        }
    }

    filemap
}

pub fn get_all_dotfiles(
    config: &DotfileConfig,
    db: &Database,
    include_root: bool,
) -> Result<HashMap<PathBuf, Dotfile>> {
    let repo_manager = DotfileRepositoryManager::new(config, db);
    let active_dirs = repo_manager.get_active_dotfile_dirs()?;
    let home_path = PathBuf::from(shellexpand::tilde("~").to_string());

    let mut all_dotfiles = Vec::new();
    for dir in active_dirs {
        let target_prefix = if dir.is_root {
            Path::new("/")
        } else {
            &home_path
        };
        let dotfiles = scan_directory_for_dotfiles(&dir.path, target_prefix, dir.is_root)?;
        all_dotfiles.push(dotfiles);
    }

    let mut merged = merge_dotfiles(all_dotfiles);

    if !include_root {
        merged.retain(|_, dotfile| !dotfile.is_root);
    }

    merged.retain(|target_path, _| !config.is_path_ignored(target_path));

    if let Ok(overrides) = crate::dot::override_config::OverrideConfig::load() {
        let _ = crate::dot::override_config::apply_overrides(&mut merged, &overrides, config);
    }

    Ok(merged)
}

pub fn filter_dotfiles_by_path<'a>(
    all_dotfiles: &'a HashMap<PathBuf, Dotfile>,
    path: &Path,
) -> Vec<&'a Dotfile> {
    all_dotfiles
        .values()
        .filter(|dotfile| dotfile.target_path.starts_with(path))
        .collect()
}

pub fn display_path(path: &Path, is_root: bool) -> String {
    if is_root {
        path.display().to_string()
    } else {
        let home = PathBuf::from(shellexpand::tilde("~").to_string());
        if let Ok(relative) = path.strip_prefix(&home) {
            format!("~/{}", relative.display())
        } else {
            path.display().to_string()
        }
    }
}

use crate::dot::dotfilerepo::DotfileDir;
