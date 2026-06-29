use crate::common::home_dir;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::repo::DotfileRepositoryManager;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Copy)]
pub enum EmptyParentBoundary {
    Home,
    HomeOrDots,
}

pub fn clean_empty_parent_dirs(path: &Path, boundary: EmptyParentBoundary) {
    let home = home_dir();
    let mut dir = path.parent();
    while let Some(parent) = dir {
        if parent.file_name().is_none()
            || parent == home
            || matches!(boundary, EmptyParentBoundary::HomeOrDots)
                && parent.file_name().is_some_and(|name| name == "dots")
        {
            break;
        }

        if parent.is_dir()
            && std::fs::read_dir(parent).is_ok_and(|mut entries| entries.next().is_none())
        {
            if std::fs::remove_dir(parent).is_err() {
                break;
            }
            dir = parent.parent();
        } else {
            break;
        }
    }
}

/// Normalize a path to use tilde notation (~/...)
/// - If path starts with ~, return as-is
/// - If path is an absolute path under home, convert to ~...
/// - Otherwise, prepend ~/ to make it relative to home
pub fn normalize_path_to_tilde(path: &str) -> String {
    if path.starts_with('~') {
        path.to_string()
    } else if path.starts_with('/') {
        let home_str = home_dir().to_string_lossy().into_owned();
        if let Some(stripped) = path.strip_prefix(&home_str) {
            if stripped.is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", stripped)
            }
        } else {
            path.to_string()
        }
    } else {
        format!("~/{}", path.trim_start_matches('/'))
    }
}

pub fn resolve_dotfile_path(path: &str, allow_root: bool, require_exists: bool) -> Result<PathBuf> {
    let home = home_dir();

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

    if require_exists && !normalized_path.exists() {
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

    let canonical_home = home
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Failed to canonicalize home directory: {}", e))?;

    let is_inside_home = if require_exists {
        normalized_path
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Failed to validate path '{}': {}", path, e))?
            .starts_with(&canonical_home)
    } else {
        normalized_path.starts_with(&canonical_home)
    };

    if !is_inside_home {
        return Err(anyhow::anyhow!(
            "Path '{}' is outside the home directory. Only files in {} are allowed.",
            normalized_path.display(),
            home.display()
        ));
    }

    if let Ok(canonical) = normalized_path.canonicalize() {
        if let Ok(relative) = canonical.strip_prefix(&canonical_home) {
            if relative.as_os_str().is_empty() {
                return Ok(home.clone());
            }
            return Ok(home.join(relative));
        }
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

            // Strip the `.age` suffix from the target path so that
            // `<dots>/.config/foo/bar.toml.age` maps to `~/.config/foo/bar.toml`.
            // `Dotfile::new` infers the encrypted kind from the source path
            // extension; the relative path used for the *target* is what
            // changes.
            let target_relative =
                crate::dot::encryption::strip_age_suffix(&relative_path).unwrap_or(relative_path);
            let target_path = target_prefix.join(target_relative);

            dotfiles.push(Dotfile::new(source_path, target_path, is_root));
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
    let home_path = home_dir();

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

    merged.retain(|target_path, _| !config.is_path_skipped(target_path));

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
        let home = home_dir();
        if let Ok(relative) = path.strip_prefix(&home) {
            format!("~/{}", relative.display())
        } else {
            path.display().to_string()
        }
    }
}

pub fn resolve_dotfile_to_source(
    config: &DotfileConfig,
    db: &Database,
    target_path: &Path,
    repo: Option<&str>,
    subdir: Option<&str>,
    include_root: bool,
) -> Result<Dotfile> {
    if repo.is_none() && subdir.is_none() {
        let all_dotfiles = get_all_dotfiles(config, db, include_root)?;
        return all_dotfiles.get(target_path).cloned().ok_or_else(|| {
            anyhow::anyhow!("no tracked dotfile found at {}", target_path.display())
        });
    }

    let repo = repo.ok_or_else(|| anyhow::anyhow!("--subdir requires --repo"))?;
    let sources = crate::dot::sources::list_sources_for_target(config, target_path)?;
    let matching: Vec<crate::dot::override_config::DotfileSource> = sources
        .into_iter()
        .filter(|source| source.repo_name == repo && subdir.is_none_or(|s| source.subdir_name == s))
        .collect();

    match matching.as_slice() {
        [] => Err(anyhow::anyhow!(
            "no tracked source found for {} in repository '{}'",
            target_path.display(),
            repo
        )),
        [source] => Ok(Dotfile::new(
            source.source_path.clone(),
            target_path.to_path_buf(),
            !target_path.starts_with(home_dir()),
        )),
        _ => Err(anyhow::anyhow!(
            "multiple sources found for {} in repository '{}'; pass --subdir",
            target_path.display(),
            repo
        )),
    }
}

pub fn persist_file_safely(path: &Path, content: &[u8], description: &str) -> Result<()> {
    use anyhow::Context;
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "{} has no parent directory: {}",
            description,
            path.display()
        )
    })?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating directory {}", parent.display()))?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary file in {}", parent.display()))?;
    tmp.write_all(content)
        .with_context(|| format!("writing temporary file for {}", path.display()))?;
    tmp.flush()?;
    tmp.persist(path)
        .map_err(|err| anyhow::anyhow!("persisting {} {}: {}", description, path.display(), err))?;
    Ok(())
}

use crate::dot::dotfilerepo::DotfileDir;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot::dotfile::SourceKind;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn scan_directory_strips_age_suffix_from_target() {
        let dir = tempdir().unwrap();
        let source_dir = dir.path().join("dots");
        let target_prefix = dir.path().join("home");
        fs::create_dir_all(source_dir.join(".config/app")).unwrap();
        fs::write(source_dir.join(".config/app/token.toml.age"), "ciphertext").unwrap();

        let dotfiles = scan_directory_for_dotfiles(&source_dir, &target_prefix, false).unwrap();

        assert_eq!(dotfiles.len(), 1);
        assert_eq!(
            dotfiles[0].source_path,
            source_dir.join(".config/app/token.toml.age")
        );
        assert_eq!(
            dotfiles[0].target_path,
            target_prefix.join(".config/app/token.toml")
        );
        assert_eq!(dotfiles[0].kind, SourceKind::Age);
    }
}
