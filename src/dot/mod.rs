use shellexpand;
use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::Result;


use walkdir::WalkDir;

pub mod config;
pub mod git;
pub mod localrepo;
pub mod meta;
pub mod dotfile;
pub mod db;

pub use crate::dot::dotfile::Dotfile;
pub use git::{add_repo, update_all, status_all};

pub fn get_all_dotfiles() -> Result<HashMap<PathBuf, Dotfile>> {
    let mut filemap = HashMap::new();
    let repos = config::Config::load()?.repos;
    let base_dir = config::repos_base_dir()?;

    for repo in repos {
        let repo_path = base_dir.join(repo.name.as_ref().unwrap());
        for entry in WalkDir::new(&repo_path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let source_path = entry.path().to_path_buf();
                let relative_path = source_path.strip_prefix(&repo_path).unwrap().to_path_buf();
                let target_path = PathBuf::from(shellexpand::tilde("~").to_string()).join(relative_path);

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