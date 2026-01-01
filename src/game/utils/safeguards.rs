use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

use crate::common::TildePath;
use crate::game::utils::path::tilde_display_string;

/// Represents how a path will be used so that error messages can be contextualized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathUsage {
    SaveDirectory,
    DependencySource,
    DependencyInstall,
}

impl PathUsage {
    fn context(self) -> &'static str {
        match self {
            PathUsage::SaveDirectory => "save",
            PathUsage::DependencySource => "dependency source",
            PathUsage::DependencyInstall => "dependency install",
        }
    }
}

struct BlockedDirectory {
    normalized: PathBuf,
    canonical: Option<PathBuf>,
    friendly_name: &'static str,
}

impl BlockedDirectory {
    fn new(path: PathBuf, friendly_name: &'static str) -> Self {
        let normalized = normalize_components(&path);
        let canonical = std::fs::canonicalize(&normalized)
            .ok()
            .map(|p| normalize_components(&p));

        Self {
            normalized,
            canonical,
            friendly_name,
        }
    }

    fn matches(&self, candidate: &Path) -> bool {
        paths_equal(candidate, &self.normalized)
            || self
                .canonical
                .as_ref()
                .map(|c| paths_equal(candidate, c))
                .unwrap_or(false)
    }
}

/// Ensure the provided path is not one of the blocked directories.
pub fn ensure_safe_path(path: &Path, usage: PathUsage) -> Result<()> {
    let normalized = normalize_to_absolute(path)?;
    let canonical = std::fs::canonicalize(&normalized)
        .ok()
        .map(|p| normalize_components(&p));

    for blocked in blocked_directories()? {
        if blocked.matches(&normalized)
            || canonical
                .as_ref()
                .map(|canonical| blocked.matches(canonical))
                .unwrap_or(false)
        {
            let display_path = tilde_display_string(&TildePath::new(normalized.clone()));
            return Err(anyhow!(
                "The {} path '{}' is not allowed because it resolves to {}.",
                usage.context(),
                display_path,
                blocked.friendly_name
            ));
        }
    }

    Ok(())
}

fn blocked_directories() -> Result<Vec<BlockedDirectory>> {
    let mut blocked = Vec::new();

    let home_dir = dirs::home_dir().ok_or_else(|| {
        anyhow!("Unable to determine the current user's home directory for path safeguards")
    })?;

    blocked.push(BlockedDirectory::new(
        home_dir.clone(),
        "your home directory (~)",
    ));

    let config_dir = dirs::config_dir().unwrap_or_else(|| home_dir.join(".config"));
    blocked.push(BlockedDirectory::new(config_dir, "~/.config"));

    blocked.push(BlockedDirectory::new(home_dir.join(".local"), "~/.local"));

    #[cfg(unix)]
    {
        blocked.push(BlockedDirectory::new(PathBuf::from("/"), "/"));
    }

    blocked.sort_by(|a, b| a.normalized.cmp(&b.normalized));
    blocked.dedup_by(|a, b| a.normalized == b.normalized);

    Ok(blocked)
}

fn normalize_to_absolute(path: &Path) -> Result<PathBuf> {
    let mut absolute = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().context("Failed to resolve relative path")?
    };

    absolute.push(path);

    Ok(normalize_components(&absolute))
}

fn normalize_components(path: &Path) -> PathBuf {
    use std::path::Component;

    let is_absolute = path.is_absolute();
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !is_absolute {
                    normalized.push("..");
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    if normalized.as_os_str().is_empty() && is_absolute {
        PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
    } else {
        normalized
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    normalize_components(a) == normalize_components(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_trailing_slash() {
        let home = dirs::home_dir().unwrap();
        let mut with_slash = home.clone();
        with_slash.push("");

        assert_eq!(
            normalize_components(&home),
            normalize_components(&with_slash)
        );
    }

    #[test]
    fn blocked_matches_canonicalized_path() {
        let home = dirs::home_dir().unwrap();
        let blocked = BlockedDirectory::new(home.clone(), "home");
        let mut alt = home.join(".");
        alt.push("..");
        alt.push(home.file_name().unwrap());

        let normalized = normalize_components(&alt);
        assert!(blocked.matches(&normalized));
    }
}
