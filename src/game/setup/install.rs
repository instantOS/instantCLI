use anyhow::{Context, Result, anyhow};
use std::fs;

use crate::dot::path_serde::TildePath;
use crate::game::checkpoint;
use crate::game::config::{GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind};
use crate::game::restic::backup::GameBackup;
use crate::game::restic::cache;
use crate::game::utils::save_files::get_save_directory_info;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

use super::paths::{
    choose_installation_path, extract_unique_paths_from_snapshots, prompt_manual_save_path,
};
use super::restic::{infer_snapshot_kind, SnapshotOverview};

/// Set up a single game by collecting paths from snapshots and letting the user choose one.
pub(super) fn setup_single_game(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
    snapshot_context: Option<&SnapshotOverview>,
) -> Result<()> {
    emit(
        Level::Info,
        "game.setup.start",
        &format!(
            "{} Setting up game: {game_name}",
            char::from(NerdFont::Info)
        ),
        None,
    );

    let (unique_paths, latest_snapshot_id, snapshot_count) = if let Some(context) = snapshot_context
    {
        (
            context.unique_paths.clone(),
            context.latest_snapshot_id.clone(),
            context.snapshot_count,
        )
    } else {
        let snapshots = cache::get_snapshots_for_game(game_name, game_config)
            .context("Failed to get snapshots for game")?;
        let latest_snapshot_id = snapshots.first().map(|snapshot| snapshot.id.clone());
        let unique_paths = if snapshots.is_empty() {
            Vec::new()
        } else {
            extract_unique_paths_from_snapshots(&snapshots)?
        };
        (unique_paths, latest_snapshot_id, snapshots.len())
    };

    if snapshot_count == 0 {
        emit(
            Level::Warn,
            "game.setup.no_snapshots",
            &format!(
                "{} No snapshots found for game '{game_name}'.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        emit(
            Level::Info,
            "game.setup.hint.add",
            &format!(
                "{} You'll be prompted to choose a save path manually.",
                char::from(NerdFont::Info)
            ),
            None,
        );
    } else if unique_paths.is_empty() {
        emit(
            Level::Warn,
            "game.setup.no_paths",
            &format!(
                "{} No save paths found in snapshots for game '{game_name}'.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        emit(
            Level::Info,
            "game.setup.hint.manual",
            &format!(
                "{} You'll be prompted to choose a save path manually.",
                char::from(NerdFont::Info)
            ),
            None,
        );
    } else {
        println!(
            "\nFound {} unique save path(s) from different devices/snapshots:",
            unique_paths.len()
        );
    }

    let chosen_path = if unique_paths.is_empty() {
        prompt_manual_save_path(game_name)?
    } else {
        choose_installation_path(game_name, &unique_paths)?
    };

    if let Some(path_str) = chosen_path {
        let save_path =
            TildePath::from_str(&path_str).map_err(|e| anyhow!("Invalid save path: {e}"))?;
        let save_path_kind =
            detect_save_path_kind(&save_path, latest_snapshot_id.as_deref(), game_config);
        let mut installation =
            GameInstallation::with_kind(game_name, save_path.clone(), save_path_kind);

        let mut path_created = false;

        if save_path_kind.is_directory() {
            if !save_path.as_path().exists() {
                match FzfWrapper::confirm(&format!(
                    "Save path '{path_str}' does not exist. Would you like to create it?"
                ))
                .map_err(|e| anyhow!("Failed to get confirmation: {e}"))?
                {
                    ConfirmResult::Yes => {
                        fs::create_dir_all(save_path.as_path())
                            .context("Failed to create save directory")?;
                        emit(
                            Level::Success,
                            "game.setup.dir_created",
                            &format!("{} Created save directory: {path_str}", char::from(NerdFont::Check)),
                            None,
                        );
                        path_created = true;
                    }
                    ConfirmResult::No | ConfirmResult::Cancelled => {
                        println!("Directory not created. You can create it later when needed.");
                    }
                }
            }
        } else {
            let path_ref = save_path.as_path();

            if path_ref.exists() && path_ref.is_dir() {
                return Err(anyhow!(
                    "Save path '{path_str}' points to a directory, but the snapshot indicates a single file save."
                ));
            }

            if !path_ref.exists() {
                if let Some(parent) = path_ref.parent() {
                    if !parent.exists() {
                        match FzfWrapper::confirm(&format!(
                            "Parent directory '{}' does not exist. Create it?",
                            parent.display()
                        ))
                        .map_err(|e| anyhow!("Failed to confirm parent directory creation: {e}"))?
                        {
                            ConfirmResult::Yes => {
                                fs::create_dir_all(parent).with_context(|| {
                                    format!("Failed to create directory '{}'", parent.display())
                                })?;
                                path_created = true;
                                emit(
                                    Level::Success,
                                    "game.setup.parent_created",
                                    &format!("{} Created parent directory: {}", char::from(NerdFont::Check), parent.display()),
                                    None,
                                );
                            }
                            ConfirmResult::No | ConfirmResult::Cancelled => {
                                println!("Parent directory not created. You can set it up later when needed.");
                            }
                        }
                    }
                }
            }
        }

        let path_exists_after = save_path.as_path().exists();
        let has_existing_snapshot = latest_snapshot_id.is_some();

        let (file_count, dir_info) = if save_path_kind.is_directory() {
            let info = get_save_directory_info(save_path.as_path())
                .with_context(|| format!("Failed to inspect save directory '{path_str}'"))?;
            (info.file_count, Some(info))
        } else {
            (if path_exists_after { 1 } else { 0 }, None)
        };

        let decision = determine_restore_decision(
            has_existing_snapshot,
            path_exists_after,
            path_created,
            file_count,
            save_path_kind,
        );
        let mut should_restore = decision.should_restore;

        if decision.needs_confirmation {
            let overwrite_prompt = if save_path_kind.is_directory() {
                let info = dir_info.as_ref().expect("directory info missing");
                format!(
                    "{} The directory '{path_str}' already contains {} file{}.\nRestoring from backup will replace its contents. Proceed?",
                    char::from(NerdFont::Warning),
                    info.file_count,
                    if info.file_count == 1 { "" } else { "s" }
                )
            } else {
                format!(
                    "{} The file '{path_str}' already exists. Restoring from backup will overwrite it. Proceed?",
                    char::from(NerdFont::Warning)
                )
            };

            match FzfWrapper::builder()
                .confirm(overwrite_prompt)
                .yes_text("Restore and overwrite")
                .no_text("Choose a different path")
                .show_confirmation()
                .map_err(|e| anyhow!("Failed to confirm restore overwrite: {e}"))?
            {
                ConfirmResult::Yes => {
                    should_restore = true;
                }
                ConfirmResult::No => {
                    if save_path_kind.is_directory() {
                        println!(
                            "{} Keeping existing files in '{path_str}'. Restore skipped.",
                            char::from(NerdFont::Info)
                        );
                    } else {
                        println!(
                            "{} Keeping existing file '{path_str}'. Restore skipped.",
                            char::from(NerdFont::Info)
                        );
                    }
                    should_restore = false;
                }
                ConfirmResult::Cancelled => {
                    emit(
                        Level::Warn,
                        "game.setup.cancelled",
                        &format!(
                            "{} Setup cancelled for game '{game_name}'.",
                            char::from(NerdFont::Warning)
                        ),
                        None,
                    );
                    return Ok(());
                }
            }
        }

        if should_restore && let Some(snapshot_id) = latest_snapshot_id.as_deref() {
            emit(
                Level::Info,
                "game.setup.restore_latest",
                &format!(
                    "{} Restoring latest backup ({snapshot_id}) into {path_str}...",
                    char::from(NerdFont::Download)
                ),
                None,
            );

            let restore_summary = restore_latest_backup(
                game_name,
                &save_path,
                snapshot_id,
                game_config,
                installation.save_path_type,
            )?;
            emit(
                Level::Success,
                "game.setup.restore_done",
                &format!("{} {restore_summary}", char::from(NerdFont::Check)),
                None,
            );
            installation.update_checkpoint(snapshot_id.to_string());
        }

        if !has_existing_snapshot {
            if path_exists_after {
                emit(
                    Level::Info,
                    "game.setup.initial_checkpoint.start",
                    &format!(
                        "{} No checkpoints found. Creating initial backup from '{path_str}'...",
                        char::from(NerdFont::Upload)
                    ),
                    None,
                );

                let backup_handler = GameBackup::new(game_config.clone());
                let backup_summary =
                    backup_handler.backup_game(&installation).with_context(|| {
                        format!("Failed to create initial checkpoint for game '{game_name}'")
                    })?;

                emit(
                    Level::Success,
                    "game.setup.initial_checkpoint.success",
                    &format!(
                        "{} Initial backup completed ({backup_summary}).",
                        char::from(NerdFont::Check)
                    ),
                    None,
                );

                if let Some(snapshot_id) = checkpoint::extract_snapshot_id_from_backup_result(
                    &backup_summary,
                    game_name,
                    game_config,
                )? {
                    installation.update_checkpoint(snapshot_id.clone());
                }

                let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
                cache::invalidate_game_cache(game_name, &repo_path);
            } else {
                emit(
                    Level::Warn,
                    "game.setup.initial_checkpoint.skipped",
                    &format!(
                        "{} Cannot create initial checkpoint because '{path_str}' does not exist.",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
            }
        }

        installations.installations.push(installation);
        installations.save()?;

        emit(
            Level::Success,
            "game.setup.success",
            &format!(
                "{} Game '{game_name}' set up successfully with save path: {path_str}",
                char::from(NerdFont::Check)
            ),
            None,
        );
    } else {
        emit(
            Level::Warn,
            "game.setup.cancelled",
            &format!(
                "{} Setup cancelled for game '{game_name}'.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
    }

    println!();
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RestoreDecision {
    should_restore: bool,
    needs_confirmation: bool,
}

fn determine_restore_decision(
    has_existing_snapshot: bool,
    path_exists: bool,
    path_created: bool,
    file_count: u64,
    save_kind: PathContentKind,
) -> RestoreDecision {
    if !has_existing_snapshot {
        return RestoreDecision {
            should_restore: false,
            needs_confirmation: false,
        };
    }

    match save_kind {
        PathContentKind::Directory => {
            if !path_exists {
                return RestoreDecision {
                    should_restore: false,
                    needs_confirmation: false,
                };
            }

            if path_created || file_count == 0 {
                return RestoreDecision {
                    should_restore: true,
                    needs_confirmation: false,
                };
            }

            RestoreDecision {
                should_restore: false,
                needs_confirmation: true,
            }
        }
        PathContentKind::File => {
            if !path_exists || path_created {
                return RestoreDecision {
                    should_restore: true,
                    needs_confirmation: false,
                };
            }

            RestoreDecision {
                should_restore: false,
                needs_confirmation: true,
            }
        }
    }
}

fn restore_latest_backup(
    game_name: &str,
    save_path: &TildePath,
    snapshot_id: &str,
    game_config: &InstantGameConfig,
    save_path_type: crate::game::config::PathContentKind,
) -> Result<String> {
    let backup_handler = GameBackup::new(game_config.clone());
    let summary = backup_handler
        .restore_backup(
            game_name,
            snapshot_id,
            save_path.as_path(),
            save_path_type,
            save_path.as_path(),
        )
        .context("Failed to restore latest backup")?;

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(game_name, &repo_path);

    Ok(summary)
}

fn detect_save_path_kind(
    save_path: &TildePath,
    latest_snapshot_id: Option<&str>,
    game_config: &InstantGameConfig,
) -> PathContentKind {
    if let Ok(metadata) = std::fs::metadata(save_path.as_path()) {
        return metadata.into();
    }

    if let Some(snapshot_id) = latest_snapshot_id {
        match infer_snapshot_kind(game_config, snapshot_id) {
            Ok(kind) => return kind,
            Err(error) => {
                emit(
                    Level::Warn,
                    "game.setup.snapshot_inspect_failed",
                    &format!(
                        "{} Could not infer save type from snapshot: {error}",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
            }
        }
    }

    PathContentKind::Directory
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_not_attempted_without_snapshots() {
        let decision = determine_restore_decision(false, true, false, 10);
        assert_eq!(
            decision,
            RestoreDecision {
                should_restore: false,
                needs_confirmation: false
            }
        );
    }

    #[test]
    fn restore_occurs_without_prompt_for_new_directories() {
        let decision = determine_restore_decision(true, true, true, 0);
        assert_eq!(
            decision,
            RestoreDecision {
                should_restore: true,
                needs_confirmation: false
            }
        );
    }

    #[test]
    fn restore_requires_confirmation_when_files_exist() {
        let decision = determine_restore_decision(true, true, false, 5);
        assert_eq!(
            decision,
            RestoreDecision {
                should_restore: false,
                needs_confirmation: true
            }
        );
    }
}
