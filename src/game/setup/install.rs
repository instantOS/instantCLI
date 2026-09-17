use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use crate::common::TildePath;
use crate::game::config::{
    GameInstallation, InstallationsConfig, InstantGameConfig, PathContentKind,
};
use crate::game::restic::cache;
use crate::game::utils::safeguards::{PathUsage, ensure_safe_path};
use crate::game::utils::save_files::get_save_directory_info;
use crate::menu::protocol;
use crate::menu_utils::{ConfirmResult, FzfSelectable, FzfWrapper, HeaderBuilder};
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
    setup_single_game_with_discovered_path(
        game_name,
        game_config,
        installations,
        snapshot_context,
        None,
    )
}

pub(super) fn setup_single_game_with_discovered_path(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
    snapshot_context: Option<&SnapshotOverview>,
    discovered_save_path: Option<&str>,
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
    snapshot_selection.announce(game_name, discovered_save_path);

    let mut discovered_save_path = discovered_save_path;
    let outcome = loop {
        let selected_path = snapshot_selection.select_path(game_name, discovered_save_path)?;

        match selected_path {
            Some(selected_path) => match finalize_game_setup(
                game_name,
                selected_path,
                game_config,
                installations,
                &snapshot_selection,
            )? {
                FinalizeOutcome::Done(outcome) => break outcome,
                FinalizeOutcome::Reselect => {
                    // A discovered path is only the initial suggestion. Reselect must
                    // let the user enter another path instead of retrying it forever.
                    discovered_save_path = None;
                    continue;
                }
            },
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
                break SetupStepOutcome::Cancelled;
            }
        }
    };

    println!();
    Ok(outcome)
}

enum FinalizeOutcome {
    Done(SetupStepOutcome),
    Reselect,
}

struct SnapshotSelection {
    unique_paths: Vec<super::paths::PathInfo>,
    latest_snapshot_id: Option<String>,
    snapshot_count: usize,
}

impl SnapshotSelection {
    fn announce(&self, game_name: &str, discovered_save_path: Option<&str>) {
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
            emit_discovered_or_manual_hint(discovered_save_path, "game.setup.hint.add");
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
            emit_discovered_or_manual_hint(discovered_save_path, "game.setup.hint.manual");
        } else {
            println!(
                "\nFound {} unique save path(s) from different devices/snapshots:",
                self.unique_paths.len()
            );
        }
    }

    fn select_path(
        &self,
        game_name: &str,
        discovered_save_path: Option<&str>,
    ) -> Result<Option<SelectedSavePath>> {
        if self.unique_paths.is_empty() {
            if let Some(discovered_save_path) = discovered_save_path {
                return Ok(Some(SelectedSavePath {
                    display_path: discovered_save_path.to_string(),
                    snapshot_path: None,
                }));
            }
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

fn emit_discovered_or_manual_hint(discovered_save_path: Option<&str>, manual_code: &str) {
    if let Some(path) = discovered_save_path {
        emit(
            Level::Info,
            "game.setup.hint.discovered_path",
            &format!(
                "{} Using discovered save path: {path}",
                char::from(NerdFont::Info)
            ),
            None,
        );
    } else {
        emit(
            Level::Info,
            manual_code,
            &format!(
                "{} You'll be prompted to choose a save path manually.",
                char::from(NerdFont::Info)
            ),
            None,
        );
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
) -> Result<FinalizeOutcome> {
    let original_selection = selected_path.display_path.clone();
    let mut save_path = TildePath::from_str(&original_selection);
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
            return Ok(FinalizeOutcome::Done(SetupStepOutcome::Cancelled));
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

    let path_prep = match prepare_save_path(&save_path, save_path_kind, &path_display)? {
        PathPreparationOutcome::Ready(prep) => prep,
        PathPreparationOutcome::Reselect => return Ok(FinalizeOutcome::Reselect),
        PathPreparationOutcome::Cancelled => {
            emit(
                Level::Warn,
                "game.setup.cancelled",
                &format!(
                    "{} Setup cancelled for game '{game_name}'.",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            return Ok(FinalizeOutcome::Done(SetupStepOutcome::Cancelled));
        }
    };
    let state = capture_path_state(&save_path, save_path_kind, &path_display)?;

    let has_existing_snapshot = snapshot_selection.latest_snapshot_id().is_some();
    use crate::game::reconciliation::{SaveChoice, apply_choice, choose_saves};
    let choice = if has_existing_snapshot {
        let snapshots = cache::get_snapshots_for_game(game_name, game_config)?;
        match choose_saves(&installation, &snapshots)? {
            SaveChoice::Cancel => return Ok(FinalizeOutcome::Done(SetupStepOutcome::Cancelled)),
            SaveChoice::Reselect => return Ok(FinalizeOutcome::Reselect),
            choice => Some(choice),
        }
    } else if path_prep.exists_after && state.file_count > 0 {
        Some(SaveChoice::Upload)
    } else {
        None
    };

    installation.sync_repository = Some(game_config.repo.as_path().to_string_lossy().to_string());
    let index = installations.installations.len();
    installations.installations.push(installation);
    if let Some(choice) = choice {
        apply_choice(game_config, installations, index, choice)?;
    } else {
        installations.save()?;
    }

    emit(
        Level::Success,
        "game.setup.success",
        &format!(
            "{} Game '{game_name}' set up successfully with save path: {path_display}",
            char::from(NerdFont::Check)
        ),
        None,
    );

    Ok(FinalizeOutcome::Done(SetupStepOutcome::Completed))
}

struct PathPreparation {
    exists_after: bool,
}

enum PathPreparationOutcome {
    Ready(PathPreparation),
    Reselect,
    Cancelled,
}

#[derive(Clone)]
struct MissingPathChoice {
    label: String,
    description: String,
    kind: MissingPathChoiceKind,
}

#[derive(Clone, Copy)]
enum MissingPathChoiceKind {
    UseAnyway,
    Reselect,
    Cancel,
}

impl FzfSelectable for MissingPathChoice {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        protocol::FzfPreview::Text(self.description.clone())
    }
}

/// After the user declines to create a missing directory, ask what they want to
/// do instead: keep the non-existent path, pick a different path, or cancel.
fn resolve_missing_path(display: &str, label: &str) -> Result<MissingPathChoiceKind> {
    let options = vec![
        MissingPathChoice {
            label: format!(
                "{} Use this path anyway (create it manually later)",
                char::from(NerdFont::Folder)
            ),
            description: format!(
                "Keep '{display}' as the {label} even though it does not exist on disk yet. You will need to create it manually before the game can use it."
            ),
            kind: MissingPathChoiceKind::UseAnyway,
        },
        MissingPathChoice {
            label: format!(
                "{} Choose a different save path",
                char::from(NerdFont::Edit)
            ),
            description:
                "Go back and select another save path (from the snapshot list or a custom one)."
                    .to_string(),
            kind: MissingPathChoiceKind::Reselect,
        },
        MissingPathChoice {
            label: format!("{} Cancel setup", char::from(NerdFont::CrossCircle)),
            description: "Abort the game setup. You can run setup again later.".to_string(),
            kind: MissingPathChoiceKind::Cancel,
        },
    ];

    match FzfWrapper::builder()
        .header(
            HeaderBuilder::new(
                NerdFont::Question,
                format!("{label} '{display}' does not exist"),
            )
            .subtitle("What would you like to do?")
            .build(),
        )
        .items(options)
        .padded()
        .select_one()
        .map_err(|e| anyhow!("Failed to prompt for missing path action: {e}"))?
    {
        crate::menu_utils::DialogOutcome::Submitted(choice) => Ok(choice.kind),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(MissingPathChoiceKind::Cancel),
    }
}

fn prepare_save_path(
    save_path: &TildePath,
    kind: PathContentKind,
    display: &str,
) -> Result<PathPreparationOutcome> {
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
                }
                ConfirmResult::Cancelled => return Ok(PathPreparationOutcome::Cancelled),
                ConfirmResult::No => match resolve_missing_path(display, "Save directory")? {
                    MissingPathChoiceKind::UseAnyway => {
                        println!("Directory not created. You can create it later when needed.");
                    }
                    MissingPathChoiceKind::Reselect => {
                        return Ok(PathPreparationOutcome::Reselect);
                    }
                    MissingPathChoiceKind::Cancel => {
                        return Ok(PathPreparationOutcome::Cancelled);
                    }
                },
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
                ConfirmResult::Cancelled => return Ok(PathPreparationOutcome::Cancelled),
                ConfirmResult::No => {
                    let parent_display = parent.display().to_string();
                    match resolve_missing_path(&parent_display, "Parent directory")? {
                        MissingPathChoiceKind::UseAnyway => {
                            println!(
                                "Parent directory not created. You can set it up later when needed."
                            );
                        }
                        MissingPathChoiceKind::Reselect => {
                            return Ok(PathPreparationOutcome::Reselect);
                        }
                        MissingPathChoiceKind::Cancel => {
                            return Ok(PathPreparationOutcome::Cancelled);
                        }
                    }
                }
            }
        }
    }

    Ok(PathPreparationOutcome::Ready(PathPreparation {
        exists_after: save_path.as_path().exists(),
    }))
}

struct PathState {
    file_count: u64,
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
        })
    } else {
        Ok(PathState {
            file_count: if save_path.as_path().exists() { 1 } else { 0 },
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

/// Let the user pick a save path during reconciliation, without any snapshot context.
pub(super) fn choose_reconciliation_path(
    game_name: &str,
) -> Result<Option<(TildePath, PathContentKind)>> {
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let selected_path = match prompt_manual_save_path(game_name, None, false)? {
        Some(selected_path) => selected_path,
        None => return Ok(None),
    };

    let save_path = TildePath::from_str(&selected_path.display_path);
    let Some(kind) =
        detect_save_path_kind(&save_path, None, &game_config, &selected_path.display_path)?
    else {
        return Ok(None);
    };

    let save_path = if kind == PathContentKind::File {
        resolve_single_file_save_path(save_path, &selected_path, None)?
    } else {
        save_path
    };

    ensure_safe_path(save_path.as_path(), PathUsage::SaveDirectory)?;

    Ok(Some((save_path, kind)))
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
        .header(
            HeaderBuilder::new(
                NerdFont::Question,
                format!("Unable to determine the save type for '{display}'"),
            )
            .subtitle("Select the appropriate save type to continue.")
            .build(),
        )
        .items(options)
        .padded()
        .select_one()?
    {
        crate::menu_utils::DialogOutcome::Submitted(option) => Ok(Some(option.kind)),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
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
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};

    #[test]
    fn missing_directory_escape_cancels_without_followup() -> Result<()> {
        for kind in [PathContentKind::Directory, PathContentKind::File] {
            let temp = tempfile::tempdir()?;
            let missing_directory = temp.path().join("missing");
            let path = if kind.is_directory() {
                missing_directory.clone()
            } else {
                missing_directory.join("save.dat")
            };
            // Leave a follow-up response queued to detect Esc incorrectly behaving as No.
            let _guard = MockQueue::new().confirm_cancelled().select_index(0).guard();
            assert!(matches!(
                prepare_save_path(&TildePath::new(path), kind, "test save path")?,
                PathPreparationOutcome::Cancelled
            ));
            assert_eq!(scripted_responses_remaining(), 1);
            assert!(!missing_directory.exists());
        }
        Ok(())
    }

    #[test]
    fn missing_directory_no_still_offers_keep_reselect_and_cancel() -> Result<()> {
        for kind in [PathContentKind::Directory, PathContentKind::File] {
            for index in 0..3 {
                let temp = tempfile::tempdir()?;
                let missing_directory = temp.path().join("missing");
                let path = if kind.is_directory() {
                    missing_directory.clone()
                } else {
                    missing_directory.join("save.dat")
                };
                let _guard = MockQueue::new().confirm_no().select_index(index).guard();
                let outcome = prepare_save_path(&TildePath::new(path), kind, "test save path")?;
                match (index, outcome) {
                    (0, PathPreparationOutcome::Ready(prep)) => {
                        assert!(!prep.exists_after);
                    }
                    (1, PathPreparationOutcome::Reselect)
                    | (2, PathPreparationOutcome::Cancelled) => {}
                    _ => panic!("Unexpected outcome for missing path choice {index}"),
                }
                assert_eq!(scripted_responses_remaining(), 0);
                assert!(!missing_directory.exists());
            }
        }
        Ok(())
    }
}
