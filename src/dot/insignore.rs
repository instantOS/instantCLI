use anyhow::{Context, Result, anyhow};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

const IGNORE_FILE_NAME: &str = ".insignore";

pub fn match_home_path(path: &Path) -> Result<Option<PathBuf>> {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    match_home_path_at(&home, path)
}

pub fn match_home_path_at(home: &Path, path: &Path) -> Result<Option<PathBuf>> {
    match_ignore_chain(home, path)
}

pub fn match_repo_path(
    repo_root: &Path,
    target_relative_path: &Path,
    is_dir: bool,
) -> Result<Option<PathBuf>> {
    let ignore_file = repo_root.join(IGNORE_FILE_NAME);
    if !ignore_file.exists() {
        return Ok(None);
    }

    let matcher = build_matcher(repo_root, &ignore_file)?;
    let matched = matcher.matched_path_or_any_parents(target_relative_path, is_dir);

    if matched.is_ignore() {
        Ok(Some(ignore_file))
    } else {
        Ok(None)
    }
}

pub fn format_skip_message(path: &Path, ignore_file: &Path) -> String {
    let home = PathBuf::from(shellexpand::tilde("~").to_string());
    let display_path = path
        .strip_prefix(&home)
        .map(|p| format!("~/{}", p.display()))
        .unwrap_or_else(|_| path.display().to_string());

    format!(
        "{} Skipping {} (ignored by {})",
        char::from(crate::ui::nerd_font::NerdFont::ArrowRight),
        display_path,
        ignore_file.display()
    )
}

fn match_ignore_chain(root: &Path, path: &Path) -> Result<Option<PathBuf>> {
    if !path.starts_with(root) {
        return Ok(None);
    }

    let is_dir = path.is_dir();
    let mut decision = None;

    for dir in ancestor_dirs(root, path, is_dir) {
        let ignore_file = dir.join(IGNORE_FILE_NAME);
        if !ignore_file.exists() {
            continue;
        }

        let matcher = build_matcher(&dir, &ignore_file)?;
        let relative_path = path.strip_prefix(&dir).unwrap_or(path);
        let matched = matcher.matched_path_or_any_parents(relative_path, is_dir);

        if matched.is_ignore() {
            decision = Some(ignore_file);
        } else if matched.is_whitelist() {
            decision = None;
        }
    }

    Ok(decision)
}

fn ancestor_dirs(root: &Path, path: &Path, is_dir: bool) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = if is_dir {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(root).to_path_buf()
    };

    loop {
        if !current.starts_with(root) {
            break;
        }

        dirs.push(current.clone());

        if current == root {
            break;
        }

        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }

    dirs.reverse();
    dirs
}

fn build_matcher(base_dir: &Path, ignore_file: &Path) -> Result<Gitignore> {
    let mut builder = GitignoreBuilder::new(base_dir);
    if let Some(err) = builder.add(ignore_file) {
        return Err(anyhow!(err))
            .with_context(|| format!("Failed to read {}", ignore_file.display()));
    }
    builder
        .build()
        .with_context(|| format!("Failed to parse {}", ignore_file.display()))
}

#[cfg(test)]
mod tests {
    use super::{match_home_path_at, match_repo_path};
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn matches_root_home_insignore() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(home.path().join(".insignore"), ".claude/settings.json\n").unwrap();
        fs::write(home.path().join(".claude/settings.json"), "{}").unwrap();

        let matched =
            match_home_path_at(home.path(), &home.path().join(".claude/settings.json")).unwrap();
        assert_eq!(matched, Some(home.path().join(".insignore")));
    }

    #[test]
    fn matches_nested_home_insignore() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join(".config/app")).unwrap();
        fs::write(home.path().join(".config/.insignore"), "secret.txt\n").unwrap();
        fs::write(home.path().join(".config/app/secret.txt"), "secret").unwrap();

        let matched =
            match_home_path_at(home.path(), &home.path().join(".config/app/secret.txt")).unwrap();
        assert_eq!(matched, Some(home.path().join(".config/.insignore")));
    }

    #[test]
    fn nested_whitelist_can_override_parent_ignore() {
        let home = tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::write(home.path().join(".insignore"), "*.json\n").unwrap();
        fs::write(home.path().join(".claude/.insignore"), "!settings.json\n").unwrap();
        fs::write(home.path().join(".claude/settings.json"), "{}").unwrap();

        let matched =
            match_home_path_at(home.path(), &home.path().join(".claude/settings.json")).unwrap();
        assert_eq!(matched, None);
    }

    #[test]
    fn matches_repo_root_insignore() {
        let repo = tempdir().unwrap();
        fs::write(repo.path().join(".insignore"), ".claude/settings.json\n").unwrap();

        let matched =
            match_repo_path(repo.path(), Path::new(".claude/settings.json"), false).unwrap();
        assert_eq!(matched, Some(repo.path().join(".insignore")));
    }
}
