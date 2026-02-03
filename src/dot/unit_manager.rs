use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::meta;
use crate::dot::repo::RepositoryManager;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitScope {
    Global,
    Repo(String),
}

impl UnitScope {
    pub fn repo_name(&self) -> Option<&str> {
        match self {
            UnitScope::Repo(name) => Some(name.as_str()),
            UnitScope::Global => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnitRepoContext {
    pub name: String,
    pub path: PathBuf,
    pub dot_dirs: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct UnitPathContext {
    pub home: PathBuf,
    pub repo: Option<UnitRepoContext>,
}

pub fn unit_display_path(unit: &str) -> String {
    let trimmed = unit.trim();
    if trimmed.starts_with('~') || trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("~/{}", trimmed.trim_start_matches('/'))
    }
}

pub fn unit_path_context_for_write(
    scope: &UnitScope,
    config: &Config,
    db: &Database,
) -> Result<UnitPathContext> {
    let home = crate::dot::sources::home_dir();
    let repo = match scope {
        UnitScope::Global => None,
        UnitScope::Repo(name) => Some(repo_context_for_write(name, config, db)?),
    };

    Ok(UnitPathContext { home, repo })
}

pub fn normalize_unit_input(path: &str, context: &UnitPathContext) -> Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Unit path cannot be empty");
    }

    let resolved = if trimmed.starts_with('~') {
        PathBuf::from(shellexpand::tilde(trimmed).to_string())
    } else if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        context.home.join(trimmed)
    };

    normalize_unit_fs_path(&resolved, context)
}

pub fn normalize_unit_fs_path(path: &Path, context: &UnitPathContext) -> Result<String> {
    if let Some(repo) = &context.repo
        && path.starts_with(&repo.path)
    {
        return normalize_repo_unit_path(path, repo);
    }

    if path.starts_with(&context.home) {
        return Ok(format_tilde_path(path, &context.home));
    }

    if let Some(repo) = &context.repo {
        anyhow::bail!(
            "Path must be within your home directory ({}) or inside the repo at {}",
            format_tilde_path(&context.home, &context.home),
            repo.path.display()
        );
    }

    anyhow::bail!(
        "Path must be within your home directory ({})",
        format_tilde_path(&context.home, &context.home)
    );
}

pub fn list_units(scope: &UnitScope, config: &Config, db: &Database) -> Result<Vec<String>> {
    match scope {
        UnitScope::Global => Ok(config.units.clone()),
        UnitScope::Repo(name) => {
            let repo_config = config
                .repos
                .iter()
                .find(|repo| repo.name == *name)
                .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", name))?;

            if let Some(meta) = &repo_config.metadata {
                return Ok(meta.units.clone());
            }

            let repo_manager = RepositoryManager::new(config, db);
            let local_repo = repo_manager
                .get_repository_info(name)
                .with_context(|| format!("Failed to load repository '{}'", name))?;
            Ok(local_repo.meta.units.clone())
        }
    }
}

pub fn add_unit(
    scope: &UnitScope,
    config: &mut Config,
    db: &Database,
    unit: &str,
    config_path: Option<&str>,
) -> Result<()> {
    match scope {
        UnitScope::Global => config.add_unit(unit.to_string(), config_path),
        UnitScope::Repo(name) => {
            let repo = repo_context_for_write(name, config, db)?;
            let mut metadata = meta::read_meta(&repo.path)
                .with_context(|| format!("Failed to read metadata for '{}'", name))?;
            if metadata.units.contains(&unit.to_string()) {
                anyhow::bail!("Path '{}' is already a unit", unit);
            }
            metadata.units.push(unit.to_string());
            meta::update_meta(&repo.path, &metadata)
                .with_context(|| format!("Failed to update metadata for '{}'", name))?;
            Ok(())
        }
    }
}

pub fn remove_unit(
    scope: &UnitScope,
    config: &mut Config,
    db: &Database,
    unit: &str,
    config_path: Option<&str>,
) -> Result<()> {
    match scope {
        UnitScope::Global => config.remove_unit(unit, config_path),
        UnitScope::Repo(name) => {
            let repo = repo_context_for_write(name, config, db)?;
            let mut metadata = meta::read_meta(&repo.path)
                .with_context(|| format!("Failed to read metadata for '{}'", name))?;
            let original_len = metadata.units.len();
            metadata.units.retain(|entry| entry != unit);
            if metadata.units.len() == original_len {
                anyhow::bail!("Path '{}' is not in the units list", unit);
            }
            meta::update_meta(&repo.path, &metadata)
                .with_context(|| format!("Failed to update metadata for '{}'", name))?;
            Ok(())
        }
    }
}

fn repo_context_for_write(
    repo_name: &str,
    config: &Config,
    db: &Database,
) -> Result<UnitRepoContext> {
    let repo_config = config
        .repos
        .iter()
        .find(|repo| repo.name == repo_name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found", repo_name))?;

    if repo_config.read_only {
        anyhow::bail!("Repository '{}' is read-only", repo_name);
    }

    if repo_config.metadata.is_some() {
        anyhow::bail!(
            "Repository '{}' is external; unit metadata editing is not supported",
            repo_name
        );
    }

    let repo_manager = RepositoryManager::new(config, db);
    let local_repo = repo_manager
        .get_repository_info(repo_name)
        .with_context(|| format!("Failed to load repository '{}'", repo_name))?;

    let repo_path = local_repo.local_path(config)?;
    let dot_dirs = local_repo
        .dotfile_dirs
        .iter()
        .map(|dir| dir.path.clone())
        .collect();

    Ok(UnitRepoContext {
        name: repo_name.to_string(),
        path: repo_path,
        dot_dirs,
    })
}

fn normalize_repo_unit_path(path: &Path, repo: &UnitRepoContext) -> Result<String> {
    let mut matches: Vec<&PathBuf> = repo
        .dot_dirs
        .iter()
        .filter(|dir| path.starts_with(dir.as_path()))
        .collect();

    if matches.is_empty() {
        let dirs = repo
            .dot_dirs
            .iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Selected path is inside '{}' but not within a dotfile directory. Choose a path inside: {}",
            repo.path.display(),
            dirs
        );
    }

    matches.sort_by_key(|dir| dir.components().count());
    let best = matches.last().unwrap();
    let relative = path.strip_prefix(best.as_path()).unwrap_or(path);
    if relative.components().count() == 0 {
        anyhow::bail!(
            "Select a directory inside the dotfile directory, not the dotfile directory root"
        );
    }

    let relative_lossy = relative.to_string_lossy();
    let relative_str = relative_lossy.trim_start_matches('/');
    if relative_str.is_empty() {
        anyhow::bail!(
            "Select a directory inside the dotfile directory, not the dotfile directory root"
        );
    }

    Ok(format!("~/{}", relative_str))
}

fn format_tilde_path(path: &Path, home: &Path) -> String {
    let relative = path.strip_prefix(home).unwrap_or(path);
    let relative_lossy = relative.to_string_lossy();
    let relative_str = relative_lossy.trim_start_matches('/');
    if relative_str.is_empty() {
        "~".to_string()
    } else {
        format!("~/{}", relative_str)
    }
}
