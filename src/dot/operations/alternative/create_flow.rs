//! Create flow for adding alternatives.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::override_config::{DotfileSource, OverrideConfig};
use crate::dot::sources;
use crate::menu_utils::{FzfResult, FzfWrapper, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::prelude::*;

use super::apply::add_to_destination;
use super::discovery::{get_destinations, to_display_path};
use super::flow::{Flow, emit_cancelled, message_and_continue, message_and_done};
use super::picker::{CreateMenuItem, SourceOption};

/// Pick a destination and add a file there (shared by `add --choose` and `alternative --create`).
pub fn pick_destination_and_add(
    config: &DotfileConfig,
    path: &Path,
    force: bool,
    config_path: Option<&str>,
) -> Result<bool> {
    let display = to_display_path(path);
    let existing = sources::list_sources_for_target(config, path)?;
    match run_create_flow(path, &display, &existing, force, config_path)? {
        Flow::Done => Ok(true),
        _ => Ok(false),
    }
}

// Create flow - adding a file to a new destination.
pub(crate) fn run_create_flow(
    path: &Path,
    display: &str,
    existing: &[DotfileSource],
    force: bool,
    config_path: Option<&str>,
) -> Result<Flow> {
    let mut cursor = MenuCursor::new();
    let mut warned_ignored_repos = HashSet::new();

    loop {
        let config = DotfileConfig::load(None)?;
        let is_root_target = !path.starts_with(crate::dot::sources::home_dir());
        let destinations: Vec<DotfileSource> = get_destinations(&config)
            .into_iter()
            .filter(|dest| {
                let is_root_dest = dest.subdir_name.ends_with("_root");
                if is_root_target != is_root_dest {
                    return false;
                }

                if force {
                    return true;
                }

                let repo_root = config.repos_path().join(&dest.repo_name);
                match crate::dot::insignore::match_repo_target_path(&repo_root, path) {
                    Ok(Some(ignore_file)) => {
                        if warned_ignored_repos.insert(dest.repo_name.clone()) {
                            println!(
                                "{}",
                                crate::dot::insignore::format_repo_skip_message(
                                    &dest.repo_name,
                                    path,
                                    &ignore_file
                                )
                            );
                        }
                        false
                    }
                    Ok(None) => true,
                    Err(err) => {
                        if warned_ignored_repos.insert(dest.repo_name.clone()) {
                            eprintln!(
                                "{} Failed to evaluate .insignore for repository '{}': {}",
                                char::from(NerdFont::Warning).to_string().yellow(),
                                dest.repo_name,
                                err
                            );
                        }
                        false
                    }
                }
            })
            .collect();

        // Build menu
        let mut menu: Vec<CreateMenuItem> = destinations
            .iter()
            .map(|dest| {
                let exists = existing
                    .iter()
                    .any(|s| s.repo_name == dest.repo_name && s.subdir_name == dest.subdir_name);
                CreateMenuItem::Destination(SourceOption {
                    source: dest.clone(),
                    is_current: false,
                    exists,
                })
            })
            .collect();

        // External repositories keep metadata in dots.toml and intentionally
        // expose a fixed structure, so they cannot create editable subdirs.
        for repo in config
            .repos
            .iter()
            .filter(|r| r.enabled && !r.read_only && !r.is_external())
        {
            menu.push(CreateMenuItem::AddSubdir {
                repo_name: repo.name.clone(),
                is_root_target,
            });
        }

        menu.push(CreateMenuItem::CloneRepo);
        menu.push(CreateMenuItem::Cancel);

        let mut builder = FzfWrapper::builder()
            .prompt(format!("Select destination for {}: ", display))
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&menu) {
            builder = builder.initial_index(index);
        }

        match builder.select(menu.clone())? {
            FzfResult::Selected(CreateMenuItem::Destination(item)) => {
                cursor.update(&CreateMenuItem::Destination(item.clone()), &menu);
                match add_file_to_destination(&config, path, display, &item, force)? {
                    Flow::Continue => continue,
                    other => return Ok(other),
                }
            }
            FzfResult::Selected(CreateMenuItem::AddSubdir {
                repo_name,
                is_root_target,
            }) => {
                cursor.update(
                    &CreateMenuItem::AddSubdir {
                        repo_name: repo_name.clone(),
                        is_root_target,
                    },
                    &menu,
                );
                if create_new_subdir(&config, &repo_name, is_root_target, config_path)? {
                    continue;
                }
                return Ok(Flow::Cancelled);
            }
            FzfResult::Selected(CreateMenuItem::CloneRepo) => {
                cursor.update(&CreateMenuItem::CloneRepo, &menu);
                if clone_new_repo()? {
                    continue;
                }
                return Ok(Flow::Cancelled);
            }
            FzfResult::Selected(CreateMenuItem::Cancel) => {
                cursor.update(&CreateMenuItem::Cancel, &menu);
                emit_cancelled();
                return Ok(Flow::Cancelled);
            }
            FzfResult::Cancelled => {
                emit_cancelled();
                return Ok(Flow::Cancelled);
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("Selection error: {}", e)),
            _ => return Ok(Flow::Cancelled),
        }
    }
}

fn add_file_to_destination(
    config: &DotfileConfig,
    path: &Path,
    display: &str,
    item: &SourceOption,
    force: bool,
) -> Result<Flow> {
    // Already exists at this destination
    if item.exists {
        return message_and_continue(&format!(
            "'{}' already exists at {} / {}\n\n\
            This location is already tracked as an alternative.\n\
            Use the alternative selection menu to switch sources.",
            display, item.source.repo_name, item.source.subdir_name
        ));
    }

    // Open database
    let db = match Database::new(config.database_path().to_path_buf()) {
        Ok(db) => db,
        Err(e) => return message_and_continue(&format!("Failed to open database: {}", e)),
    };

    // Copy the file
    let added = match add_to_destination(config, &db, path, &item.source, force, None) {
        Ok(added) => added,
        Err(e) => {
            return message_and_continue(&format!(
                "Failed to add '{}' to {} / {}:\n\n{}",
                display, item.source.repo_name, item.source.subdir_name, e
            ));
        }
    };
    if !added {
        return message_and_continue(&format!(
            "'{}' was skipped because it is ignored.\n\nUse '--force' to add it anyway.",
            display
        ));
    }

    // Check how many sources exist now
    let config = DotfileConfig::load(None)?;
    let sources = sources::list_sources_for_target(&config, path)?;

    if sources.len() <= 1 {
        // Only one source - just tracking, no override needed
        return message_and_done(&format!(
            "Added '{}' to {} / {}\n\n\
            Note: This file is now tracked, but has no alternatives.\n\
            An override is only needed when multiple sources exist.",
            display, item.source.repo_name, item.source.subdir_name
        ));
    }

    // Multiple sources - set override
    let mut overrides = match OverrideConfig::load() {
        Ok(o) => o,
        Err(e) => {
            return message_and_done(&format!(
                "File was copied but failed to load overrides: {}\n\n\
                Use 'ins dot alternative {}' to switch sources.",
                e, display
            ));
        }
    };

    if let Err(e) = overrides.set_override(
        path.to_path_buf(),
        item.source.repo_name.clone(),
        item.source.subdir_name.clone(),
    ) {
        return message_and_done(&format!(
            "File was copied but failed to set override: {}\n\n\
            Use 'ins dot alternative {}' to switch sources.",
            e, display
        ));
    }

    message_and_done(&format!(
        "Created alternative for '{}' at {} / {}\n\n\
        This location is now set as the active source.\n\
        {} source(s) available.",
        display,
        item.source.repo_name,
        item.source.subdir_name,
        sources.len()
    ))
}

fn create_new_subdir(
    config: &DotfileConfig,
    repo_name: &str,
    is_root_target: bool,
    config_path: Option<&str>,
) -> Result<bool> {
    let mut new_dir = match FzfWrapper::builder()
        .prompt(if is_root_target {
            "New root dotfile directory name (_root is added automatically): "
        } else {
            "New dotfile directory name: "
        })
        .args(fzf_mocha_args())
        .input()
        .input_result()?
    {
        FzfResult::Selected(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Ok(false),
    };

    if is_root_target && !new_dir.ends_with("_root") {
        new_dir.push_str("_root");
    }

    match create_and_activate_subdir(config, repo_name, &new_dir, config_path) {
        Ok(()) => {
            emit(
                Level::Success,
                "dot.alternative.subdir_created",
                &format!(
                    "{} Created dotfile directory '{}/{}' - now select it",
                    char::from(NerdFont::Check),
                    repo_name.green(),
                    new_dir.green()
                ),
                None,
            );
            Ok(true)
        }
        Err(e) => {
            FzfWrapper::message(&format!("Failed to create directory: {}", e))?;
            Ok(false)
        }
    }
}

pub(crate) fn create_and_activate_subdir(
    config: &DotfileConfig,
    repo_name: &str,
    new_dir: &str,
    config_path: Option<&str>,
) -> Result<()> {
    use crate::dot::dotfilerepo::DotfileRepo;
    use anyhow::Context;
    use std::fs;

    let dotfile_repo = DotfileRepo::new(config, repo_name.to_string())?;
    if dotfile_repo.is_external(config) {
        anyhow::bail!(
            "Repository '{}' is external; dotfile directory creation is not supported",
            repo_name
        );
    }

    let local_path = dotfile_repo.local_path(config)?;
    let original_meta = dotfile_repo.meta.clone();
    let new_dir_path = local_path.join(new_dir);
    let new_dir_existed = new_dir_path.exists();
    crate::dot::meta::add_dots_dir(&local_path, new_dir)?;

    let mut config = config.clone();
    if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name) {
        let active_subdirs = repo.active_subdirectories.get_or_insert_with(Vec::new);
        if !active_subdirs.contains(&new_dir.to_string()) {
            active_subdirs.push(new_dir.to_string());
            if let Err(save_err) = config.save(config_path) {
                let rollback_result = crate::dot::meta::update_meta(&local_path, &original_meta)
                    .and_then(|()| {
                        if !new_dir_existed {
                            fs::remove_dir(&new_dir_path).with_context(|| {
                                format!("removing directory {}", new_dir_path.display())
                            })?;
                        }
                        Ok(())
                    });
                return match rollback_result {
                    Ok(()) => Err(save_err).context("activating new dotfile directory"),
                    Err(rollback_err) => Err(save_err).context(format!(
                        "activating new dotfile directory; rollback also failed: {}",
                        rollback_err
                    )),
                };
            }
        }
    }

    if let Err(e) =
        crate::dot::git::repo_ops::git_add(&local_path, &local_path.join("instantdots.toml"), false)
    {
        eprintln!(
            "{} Failed to stage instantdots.toml: {}",
            char::from(NerdFont::Warning).to_string().yellow(),
            e
        );
    }

    Ok(())
}

fn clone_new_repo() -> Result<bool> {
    let mut config = DotfileConfig::load(None)?;
    let db = Database::new(config.database_path().to_path_buf())?;
    let original_count = config.repos.len();

    crate::dot::menu::add_repo::handle_add_repo(&mut config, &db, false)?;

    Ok(config.repos.len() > original_count)
}

#[cfg(test)]
mod tests {
    use super::create_and_activate_subdir;
    use crate::common::TildePath;
    use crate::dot::config::{DotfileConfig, Repo};
    use crate::dot::meta;
    use crate::dot::types::RepoMetaData;
    use std::fs;

    fn setup_repo() -> (tempfile::TempDir, DotfileConfig) {
        let dir = tempfile::tempdir().unwrap();
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("personal");
        fs::create_dir_all(repo_dir.join("dots")).unwrap();
        fs::write(
            repo_dir.join("instantdots.toml"),
            "name = \"personal\"\ndots_dirs = [\"dots\"]\n",
        )
        .unwrap();
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(&repo_dir)
            .status()
            .unwrap();

        let config = DotfileConfig {
            repos: vec![Repo {
                url: "local".to_string(),
                name: "personal".to_string(),
                branch: None,
                active_subdirectories: None,
                enabled: true,
                read_only: false,
                metadata: None,
            }],
            repos_dir: TildePath::new(repos_dir),
            database_dir: TildePath::new(dir.path().join("instant.db")),
            ..DotfileConfig::default()
        };
        (dir, config)
    }

    #[test]
    fn creating_subdir_activates_it_in_custom_config() {
        let (dir, config) = setup_repo();
        let config_path = dir.path().join("custom-dots.toml");

        create_and_activate_subdir(&config, "personal", "dots_root", config_path.to_str()).unwrap();

        let saved = DotfileConfig::load(config_path.to_str()).unwrap();
        assert_eq!(
            saved.repos[0].active_subdirectories.as_deref(),
            Some(&["dots_root".to_string()][..])
        );
        assert!(
            meta::read_meta(&config.repos_path().join("personal"))
                .unwrap()
                .dots_dirs
                .contains(&"dots_root".to_string())
        );
    }

    #[test]
    fn creating_subdir_rolls_back_metadata_when_activation_save_fails() {
        let (dir, config) = setup_repo();
        let invalid_config_path = dir.path().join("config-dir");
        fs::create_dir(&invalid_config_path).unwrap();

        let result = create_and_activate_subdir(
            &config,
            "personal",
            "dots_root",
            invalid_config_path.to_str(),
        );

        assert!(result.is_err());
        let repo_path = config.repos_path().join("personal");
        assert_eq!(meta::read_meta(&repo_path).unwrap().dots_dirs, ["dots"]);
        assert!(!repo_path.join("dots_root").exists());
    }

    #[test]
    fn external_repo_cannot_create_dotfile_subdir() {
        let (dir, mut config) = setup_repo();
        config.repos[0].metadata = Some(RepoMetaData {
            name: "personal".to_string(),
            dots_dirs: vec![".".to_string()],
            ..RepoMetaData::default()
        });
        let config_path = dir.path().join("custom-dots.toml");

        let result =
            create_and_activate_subdir(&config, "personal", "dots_root", config_path.to_str());

        assert!(result.unwrap_err().to_string().contains("is external"));
        assert!(!config.repos_path().join("personal/dots_root").exists());
    }
}
