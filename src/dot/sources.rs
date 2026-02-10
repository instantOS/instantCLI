use crate::dot::config::DotfileConfig;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::override_config::DotfileSource;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn home_dir() -> PathBuf {
    PathBuf::from(shellexpand::tilde("~").to_string())
}

pub fn default_source_for(sources: &[DotfileSource]) -> Option<DotfileSource> {
    sources.first().cloned()
}

pub fn list_sources_for_target(config: &DotfileConfig, target_path: &Path) -> Result<Vec<DotfileSource>> {
    let home = home_dir();
    let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
    let mut sources = Vec::new();

    for repo_config in &config.repos {
        if !repo_config.enabled {
            continue;
        }

        let dotfile_repo = match DotfileRepo::new(config, repo_config.name.clone()) {
            Ok(repo) => repo,
            Err(_) => continue,
        };

        for dotfile_dir in dotfile_repo.active_dotfile_dirs() {
            let source_path = dotfile_dir.path.join(relative_path);
            if source_path.exists() {
                let subdir_name = dotfile_dir
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                sources.push(DotfileSource {
                    repo_name: repo_config.name.clone(),
                    subdir_name,
                    source_path,
                });
            }
        }
    }

    Ok(sources)
}

pub fn list_sources_by_target_in_dir(
    config: &DotfileConfig,
    dir_path: &Path,
) -> Result<HashMap<PathBuf, Vec<DotfileSource>>> {
    let home = home_dir();
    let mut sources_by_target: HashMap<PathBuf, Vec<DotfileSource>> = HashMap::new();

    for repo_config in &config.repos {
        if !repo_config.enabled {
            continue;
        }

        let dotfile_repo = match DotfileRepo::new(config, repo_config.name.clone()) {
            Ok(repo) => repo,
            Err(_) => continue,
        };

        for dotfile_dir in dotfile_repo.active_dotfile_dirs() {
            let subdir_name = dotfile_dir
                .path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            for entry in WalkDir::new(&dotfile_dir.path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    !e.path().to_string_lossy().contains("/.git/") && e.file_type().is_file()
                })
            {
                let source_path = entry.path().to_path_buf();
                let relative_path = match source_path.strip_prefix(&dotfile_dir.path) {
                    Ok(rel) => rel,
                    Err(_) => continue,
                };
                let target_path = home.join(relative_path);

                if !target_path.starts_with(dir_path) {
                    continue;
                }

                sources_by_target
                    .entry(target_path)
                    .or_default()
                    .push(DotfileSource {
                        repo_name: repo_config.name.clone(),
                        subdir_name: subdir_name.clone(),
                        source_path,
                    });
            }
        }
    }

    Ok(sources_by_target)
}
