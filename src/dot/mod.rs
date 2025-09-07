use anyhow::Result;
use shellexpand;
use std::collections::HashMap;
use std::path::PathBuf;

use walkdir::WalkDir;

pub mod config;
pub mod db;
pub mod dotfile;
pub mod git;
pub mod localrepo;
pub mod meta;

pub use crate::dot::dotfile::Dotfile;
pub use git::{add_repo, status_all, update_all};

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
        for entry in WalkDir::new(&repo_path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let source_path = entry.path().to_path_buf();
                let relative_path = source_path.strip_prefix(&repo_path).unwrap().to_path_buf();
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
