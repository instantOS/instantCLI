use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use crate::common::TildePath;
use crate::game::checkpoint;
use crate::game::config::{
    GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind,
};
use crate::game::restic::backup::{GameBackup, RestoreRequest};
use crate::game::restic::cache;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::game::utils::save_files::{SaveDirectoryInfo, get_save_directory_info};
use crate::menu::protocol;
use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

use super::paths::{
    SelectedSavePath, choose_installation_path, extract_unique_paths_from_snapshots,
    prompt_manual_save_path,
};
use super::restic::{SnapshotOverview, infer_snapshot_kind};

/// Set up a single game by collecting paths from snapshots and letting the user choose one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStepOutcome {
    Completed,
    Cancelled,
}

pub(super) fn setup_single_game(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
    snapshot_context: Option<&SnapshotOverview>,
) -> Result<SetupStepOutcome> {
    emit(
        Level::Info,
        "game.setup.start",
        &format!(
            "{} Setting up game: {game_name}",
            char::from(NerdFont::Info)
        ),
        None,
    );

    let snapshot_selection = gather_snapshot_selection(game_name, game_config, snapshot_context)?;
    snapshot_selection.announce(game_name);

    let outcome = match snapshot_selection.select_path(game_name)? {
        Some(selected_path) => finalize_game_setup(
            game_name,
            selected_path,
            game_config,
            installations,
            &snapshot_selection,
        )?,
        None => {
            emit(
                Level::Warn,
                "game.setup.cancelled",
                &format!(
                    "{} Setup cancelled for game '{game_name}'.",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            SetupStepOutcome::Cancelled
        }
    };

    println!();
    Ok(outcome)
}

struct SnapshotSelection {
    unique_paths: Vec<super::paths::PathInfo>,
    latest_snapshot_id: Option<String>,
    snapshot_count: usize,
}

impl SnapshotSelection {
    fn announce(&self, game_name: &str) {
        if self.snapshot_count == 0 {
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
        } else if self.unique_paths.is_empty() {
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
                self.unique_paths.len()
            );
        }
    }

    fn select_path(&self, game_name: &str) -> Result<Option<SelectedSavePath>> {
        if self.unique_paths.is_empty() {
            prompt_manual_save_path(game_name, None, false)
        } else {
            choose_installation_path(game_name, &self.unique_paths, None)
        }
    }

    fn latest_snapshot_id(&self) -> Option<&str> {
        self.latest_snapshot_id.as_deref()
    }

    fn snapshot_file_name(&self) -> Option<OsString> {
        for path_info in &self.unique_paths {
            for snapshot_path in &path_info.snapshot_paths {
                if let Some(name) = Path::new(snapshot_path).file_name() {
                    return Some(name.to_os_string());
                }
            }
        }
        None
    }
}

fn gather_snapshot_selection(
    game_name: &str,
    game_config: &InstantGameConfig,
    snapshot_context: Option<&SnapshotOverview>,
) -> Result<SnapshotSelection> {
    if let Some(context) = snapshot_context {
        return Ok(SnapshotSelection {
            unique_paths: context.unique_paths.clone(),
            latest_snapshot_id: context.latest_snapshot_id.clone(),
            snapshot_count: context.snapshot_count,
        });
    }

    let snapshots = cache::get_snapshots_for_game(game_name, game_config)
        .context("Failed to get snapshots for game")?;
    let latest_snapshot_id = snapshots.first().map(|snapshot| snapshot.id.clone());
    let unique_paths = if snapshots.is_empty() {
        Vec::new()
    } else {
        extract_unique_paths_from_snapshots(&snapshots)?
    };

    Ok(SnapshotSelection {
        unique_paths,
        latest_snapshot_id,
        snapshot_count: snapshots.len(),
    })
}

fn finalize_game_setup(
    game_name: &str,
    selected_path: SelectedSavePath,
    game_config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
    snapshot_selection: &SnapshotSelection,
) -> Result<SetupStepOutcome> {
    let original_selection = selected_path.display_path.clone();
    let mut save_path =
        TildePath::from_str(&original_selection).map_err(|e| anyhow!("Invalid save path: {e}"))?;
    let snapshot_kind = snapshot_selection
        .latest_snapshot_id()
        .and_then(|id| infer_snapshot_kind(game_config, id).ok());

    let mut save_path_kind = match detect_save_path_kind(
        &save_path,
        snapshot_selection.latest_snapshot_id(),
        game_config,
        &original_selection,
    )? {
        Some(kind) => kind,
        None => {
            emit(
                Level::Warn,
                "game.setup.cancelled",
                &format!(
                    "{} Setup cancelled for game '{game_name}'.",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            return Ok(SetupStepOutcome::Cancelled);
        }
    };
    if save_path_kind == PathContentKind::Directory
        && matches!(snapshot_kind, Some(PathContentKind::File))
    {
        save_path_kind = PathContentKind::File;
    }
    if save_path_kind == PathContentKind::File {
        let snapshot_file_name = snapshot_selection.snapshot_file_name();
        save_path = resolve_single_file_save_path(save_path, &selected_path, snapshot_file_name)?;
    }

    ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory)?;

    let path_display = save_path
        .to_tilde_string()
        .unwrap_or_else(|_| save_path.as_path().to_string_lossy().to_string());

    let mut installation =
        GameInstallation::with_kind(game_name, save_path.clone(), save_path_kind);

    let path_prep = prepare_save_path(&save_path, save_path_kind, &path_display)?;
    let state = capture_path_state(&save_path, save_path_kind, &path_display)?;

    let has_existing_snapshot = snapshot_selection.latest_snapshot_id().is_some();
    let decision = determine_restore_decision(
        has_existing_snapshot,
        path_prep.exists_after,
        path_prep.path_created,
        state.file_count,
        save_path_kind,
    );

    let should_restore = match resolve_restore_decision(
        game_name,
        &path_display,
        save_path_kind,
        decision,
        state.directory_info.as_ref(),
    )? {
        RestoreFlow::Cancelled => return Ok(SetupStepOutcome::Cancelled),
        RestoreFlow::Proceed(value) => value,
    };

    if should_restore && let Some(snapshot_id) = snapshot_selection.latest_snapshot_id() {
        emit(
            Level::Info,
            "game.setup.restore_latest",
            &format!(
                "{} Restoring latest backup ({snapshot_id}) into {path_display}...",
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
            selected_path.snapshot_path.as_deref(),
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
        handle_initial_checkpoint(
            game_name,
            &path_display,
            game_config,
            &mut installation,
            path_prep.exists_after,
        )?;
    }

    installations.installations.push(installation);
    installations.save()?;

    emit(
        Level::Success,
        "game.setup.success",
        &format!(
            "{} Game '{game_name}' set up successfully with save path: {path_display}",
            char::from(NerdFont::Check)
        ),
        None,
    );

    Ok(SetupStepOutcome::Completed)
}

struct PathPreparation {
    path_created: bool,
    exists_after: bool,
}

fn prepare_save_path(
    save_path: &TildePath,
    kind: PathContentKind,
    display: &str,
) -> Result<PathPreparation> {
    let mut path_created = false;

    if kind.is_directory() {
        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "Save path '{display}'\ndoes not exist. Would you like to create it?"
            ))
            .map_err(|e| anyhow!("Failed to get confirmation: {e}"))?
            {
                ConfirmResult::Yes => {
                    fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    emit(
                        Level::Success,
                        "game.setup.dir_created",
                        &format!(
                            "{} Created save directory: {display}",
                            char::from(NerdFont::Check)
                        ),
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
                "Save path '{display}' points to a directory, but the snapshot indicates a single file save."
            ));
        }

        if !path_ref.exists()
            && let Some(parent) = path_ref.parent()
            && !parent.exists()
        {
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
                        &format!(
                            "{} Created parent directory: {}",
                            char::from(NerdFont::Check),
                            parent.display()
                        ),
                        None,
                    );
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Parent directory not created. You can set it up later when needed.");
                }
            }
        }
    }

    Ok(PathPreparation {
        path_created,
        exists_after: save_path.as_path().exists(),
    })
}

struct PathState {
    file_count: u64,
    directory_info: Option<SaveDirectoryInfo>,
}

fn capture_path_state(
    save_path: &TildePath,
    kind: PathContentKind,
    display: &str,
) -> Result<PathState> {
    if kind.is_directory() {
        let info = get_save_directory_info(save_path.as_path())
            .with_context(|| format!("Failed to inspect save directory '{display}'"))?;
        Ok(PathState {
            file_count: info.file_count,
            directory_info: Some(info),
        })
    } else {
        Ok(PathState {
            file_count: if save_path.as_path().exists() { 1 } else { 0 },
            directory_info: None,
        })
    }
}

fn resolve_single_file_save_path(
    save_path: TildePath,
    selected_path: &SelectedSavePath,
    snapshot_file_name: Option<OsString>,
) -> Result<TildePath> {
    if save_path.as_path().is_dir() {
        let dir = save_path.as_path();
        let mut fallback = snapshot_file_name;
        let file_name = selected_path
            .snapshot_path
            .as_deref()
            .and_then(|snapshot| Path::new(snapshot).file_name())
            .map(|name| name.to_os_string())
            .or_else(|| fallback.take())
            .ok_or_else(|| {
                let display = save_path
                    .to_tilde_string()
                    .unwrap_or_else(|_| dir.to_string_lossy().to_string());
                anyhow!(
                    "The selected path '{display}' is a directory. Please provide a full file path for single-file saves."
                )
            })?;
        let final_path = dir.join(file_name);
        Ok(TildePath::new(final_path))
    } else {
        Ok(save_path)
    }
}

enum RestoreFlow {
    Proceed(bool),
    Cancelled,
}

fn resolve_restore_decision(
    game_name: &str,
    display: &str,
    kind: PathContentKind,
    decision: RestoreDecision,
    directory_info: Option<&SaveDirectoryInfo>,
) -> Result<RestoreFlow> {
    if !decision.needs_confirmation {
        return Ok(RestoreFlow::Proceed(decision.should_restore));
    }

    let prompt = if kind.is_directory() {
        let info = directory_info
            .ok_or_else(|| anyhow!("Directory information missing for restore confirmation"))?;
        format!(
            "{} The directory '{display}' already contains {} file{}.\nRestoring from backup will replace its contents. Proceed?",
            char::from(NerdFont::Warning),
            info.file_count,
            if info.file_count == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} The file '{display}' already exists. Restoring from backup will overwrite it. Proceed?",
            char::from(NerdFont::Warning)
        )
    };

    match FzfWrapper::builder()
        .confirm(prompt)
        .yes_text("Restore and overwrite")
        .no_text("Choose a different path")
        .show_confirmation()
        .map_err(|e| anyhow!("Failed to confirm restore overwrite: {e}"))?
    {
        ConfirmResult::Yes => Ok(RestoreFlow::Proceed(true)),
        ConfirmResult::No => {
            if kind.is_directory() {
                println!(
                    "{} Keeping existing files in '{display}'. Restore skipped.",
                    char::from(NerdFont::Info)
                );
            } else {
                println!(
                    "{} Keeping existing file '{display}'. Restore skipped.",
                    char::from(NerdFont::Info)
                );
            }
            Ok(RestoreFlow::Proceed(false))
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
            Ok(RestoreFlow::Cancelled)
        }
    }
}

fn handle_initial_checkpoint(
    game_name: &str,
    display: &str,
    game_config: &InstantGameConfig,
    installation: &mut GameInstallation,
    path_exists_after: bool,
) -> Result<()> {
    if !path_exists_after {
        emit(
            Level::Warn,
            "game.setup.initial_checkpoint.skipped",
            &format!(
                "{} Cannot create initial checkpoint because '{display}' does not exist.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(());
    }

    emit(
        Level::Info,
        "game.setup.initial_checkpoint.start",
        &format!(
            "{} No checkpoints found. Creating initial backup from '{display}'...",
            char::from(NerdFont::Upload)
        ),
        None,
    );

    let backup_handler = GameBackup::new(game_config.clone());
    let backup_summary = backup_handler
        .backup_game(installation)
        .with_context(|| format!("Failed to create initial checkpoint for game '{game_name}'"))?;

    emit(
        Level::Success,
        "game.setup.initial_checkpoint.success",
        &format!(
            "{} Initial backup completed ({backup_summary}).",
            char::from(NerdFont::Check)
        ),
        None,
    );

    if let Some(snapshot_id) =
        checkpoint::extract_snapshot_id(&backup_summary, game_name, game_config)?
    {
        installation.update_checkpoint(snapshot_id.clone());
    }

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(game_name, &repo_path);

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
    save_path_type: PathContentKind,
    snapshot_source_path: Option<&str>,
) -> Result<String> {
    let backup_handler = GameBackup::new(game_config.clone());
    let summary = backup_handler
        .restore_backup(RestoreRequest {
            game_name,
            snapshot_id,
            path: save_path.as_path(),
            save_path_type,
            snapshot_source_path,
        })
        .context("Failed to restore latest backup")?;

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    cache::invalidate_game_cache(game_name, &repo_path);

    Ok(summary)
}

fn detect_save_path_kind(
    save_path: &TildePath,
    latest_snapshot_id: Option<&str>,
    game_config: &InstantGameConfig,
    display: &str,
) -> Result<Option<PathContentKind>> {
    if let Ok(metadata) = fs::metadata(save_path.as_path()) {
        return Ok(Some(metadata.into()));
    }

    if let Some(snapshot_id) = latest_snapshot_id {
        match infer_snapshot_kind(game_config, snapshot_id) {
            Ok(kind) => return Ok(Some(kind)),
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

    prompt_save_path_kind(display)
}

fn prompt_save_path_kind(display: &str) -> Result<Option<PathContentKind>> {
    let options = vec![
        SavePathKindOption::new(
            format!(
                "{} Directory containing multiple files",
                char::from(NerdFont::Folder)
            ),
            format!(
                "Choose this if '{display}' resolves to a folder of save data (multiple files or subdirectories)."
            ),
            PathContentKind::Directory,
        ),
        SavePathKindOption::new(
            format!("{} Single save file", char::from(NerdFont::File)),
            format!(
                "Choose this if '{display}' is a single file created by the game (e.g., *.sav, *.slot)."
            ),
            PathContentKind::File,
        ),
    ];

    match FzfWrapper::builder()
        .prompt("save-type")
        .header(format!(
            "{} Unable to determine the save type for '{display}'.\nSelect the appropriate save type to continue.",
            char::from(NerdFont::Question)
        ))
        .select(options)?
    {
        FzfResult::Selected(option) => Ok(Some(option.kind)),
        FzfResult::MultiSelected(mut options) => Ok(options.pop().map(|opt| opt.kind)),
        FzfResult::Cancelled => Ok(None),
        FzfResult::Error(err) => Err(anyhow!(err)),
    }
}

#[derive(Clone)]
struct SavePathKindOption {
    label: String,
    description: String,
    kind: PathContentKind,
}

impl SavePathKindOption {
    fn new(label: String, description: String, kind: PathContentKind) -> Self {
        Self {
            label,
            description,
            kind,
        }
    }
}

impl FzfSelectable for SavePathKindOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        protocol::FzfPreview::Text(self.description.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_not_attempted_without_snapshots() {
        let decision =
            determine_restore_decision(false, true, false, 10, PathContentKind::Directory);
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
        let decision = determine_restore_decision(true, true, true, 0, PathContentKind::Directory);
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
        let decision = determine_restore_decision(true, true, false, 5, PathContentKind::Directory);
        assert_eq!(
            decision,
            RestoreDecision {
                should_restore: false,
                needs_confirmation: true
            }
        );
    }

    #[test]
    fn restore_occurs_for_single_file_when_missing() {
        let decision = determine_restore_decision(true, false, false, 0, PathContentKind::File);
        assert_eq!(
            decision,
            RestoreDecision {
                should_restore: true,
                needs_confirmation: false
            }
        );
    }

    #[test]
    fn restore_prompts_when_single_file_exists() {
        let decision = determine_restore_decision(true, true, false, 1, PathContentKind::File);
        assert_eq!(
            decision,
            RestoreDecision {
                should_restore: false,
                needs_confirmation: true
            }
        );
    }
}
