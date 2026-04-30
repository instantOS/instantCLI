use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::commands::{
    add_scan_directory, change_scan_directory_path, configure_scan_directory_extensions,
    edit_config, remove_scan_directory, resolve_conflicts, resolve_duplicates, resolved_config,
};
use super::config::{ResolvedScanDir, ResolvethingConfig, format_path};
use super::duplicates::GroupPlan;

/// Outcome of an inner action: keep the inner loop running or return to the
/// outer menu.
enum ActionResult {
    Stay,
    Back,
}

/// Top-level menu entry: each configured scan_dir is its own entry, plus a
/// few global actions modeled on `ins game menu`.
#[derive(Debug, Clone)]
enum TopEntry {
    ScanDir {
        index: usize,
        resolved: ResolvedScanDir,
        status: ScanDirStatus,
    },
    ResolveAll,
    AddScanDir,
    EditConfig,
    Close,
}

/// Inner per-scan-dir action menu entry.
#[derive(Debug, Clone)]
enum ScanDirAction {
    ResolveEverything,
    ResolveDuplicates,
    ResolveConflicts,
    OpenDirectory,
    EditExtensions,
    ChangePath,
    Remove,
    Back,
}

#[derive(Debug, Clone)]
struct ScanDirStatus {
    duplicate_count: Option<usize>,
    conflict_count: Option<usize>,
    warnings: Vec<String>,
    exists: bool,
}

#[derive(Clone)]
struct TopItem {
    entry: TopEntry,
    preview: String,
}

#[derive(Clone)]
struct ActionItem {
    action: ScanDirAction,
    preview: String,
}

impl FzfSelectable for TopItem {
    fn fzf_display_text(&self) -> String {
        match &self.entry {
            TopEntry::ScanDir {
                resolved, status, ..
            } => {
                let dup = status
                    .duplicate_count
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let cnf = status
                    .conflict_count
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let icon_color = if status.exists {
                    colors::TEAL
                } else {
                    colors::RED
                };
                format!(
                    "{} {}  [dups: {}, conflicts: {}]",
                    format_icon_colored(NerdFont::Folder, icon_color),
                    resolved.display_path(),
                    dup,
                    cnf,
                )
            }
            TopEntry::ResolveAll => format!(
                "{} Resolve All Scan Dirs",
                format_icon_colored(NerdFont::Sync, colors::GREEN)
            ),
            TopEntry::AddScanDir => format!(
                "{} Add Scan Directory",
                format_icon_colored(NerdFont::Plus, colors::MAUVE)
            ),
            TopEntry::EditConfig => format!(
                "{} Edit Config File",
                format_icon_colored(NerdFont::Edit, colors::BLUE)
            ),
            TopEntry::Close => format!("{} Close Menu", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match &self.entry {
            TopEntry::ScanDir { index, .. } => format!("scan_dir:{}", index),
            TopEntry::ResolveAll => "!__resolve_all__".to_string(),
            TopEntry::AddScanDir => "!__add_scan_dir__".to_string(),
            TopEntry::EditConfig => "!__edit_config__".to_string(),
            TopEntry::Close => "!__close__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(self.preview.clone())
    }
}

impl FzfSelectable for ActionItem {
    fn fzf_display_text(&self) -> String {
        match self.action {
            ScanDirAction::ResolveEverything => format!(
                "{} Resolve Everything",
                format_icon_colored(NerdFont::Sync, colors::GREEN)
            ),
            ScanDirAction::ResolveDuplicates => format!(
                "{} Resolve Duplicates",
                format_icon_colored(NerdFont::File, colors::MAUVE)
            ),
            ScanDirAction::ResolveConflicts => format!(
                "{} Resolve Conflicts",
                format_icon_colored(NerdFont::GitCompare, colors::PEACH)
            ),
            ScanDirAction::OpenDirectory => format!(
                "{} Open Directory",
                format_icon_colored(NerdFont::FolderOpen, colors::TEAL)
            ),
            ScanDirAction::EditExtensions => format!(
                "{} Edit Extensions",
                format_icon_colored(NerdFont::Edit, colors::YELLOW)
            ),
            ScanDirAction::ChangePath => format!(
                "{} Change Path",
                format_icon_colored(NerdFont::Folder, colors::BLUE)
            ),
            ScanDirAction::Remove => format!(
                "{} Remove Scan Directory",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            ScanDirAction::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        format!("{:?}", self.action)
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(self.preview.clone())
    }
}

pub fn resolvething_menu(debug: bool) -> Result<()> {
    let _ = debug;
    let mut cursor = MenuCursor::new();
    // Statuses are computed once per "dirty" cycle (initial render + after any
    // resolve action) instead of on every loop iteration, avoiding a full
    // fclones scan on every menu navigation.
    let mut cached_statuses: Vec<ScanDirStatus> = Vec::new();
    let mut statuses_dirty = true;

    loop {
        let config = resolved_config()?;

        if statuses_dirty || cached_statuses.len() != config.scan_dirs.len() {
            cached_statuses = compute_all_statuses(&config);
            statuses_dirty = false;
        }

        let entries = build_top_entries(&config, &cached_statuses);
        let items: Vec<TopItem> = entries
            .iter()
            .map(|entry| TopItem {
                entry: entry.clone(),
                preview: build_top_preview(entry, &config),
            })
            .collect();

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("Resolvething"))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&items) {
            builder = builder.initial_index(index);
        }

        let selected = match builder.select(items.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &items);
                item.entry
            }
            _ => return Ok(()),
        };

        match selected {
            TopEntry::ScanDir { index, .. } => {
                let (_, did_resolve) = run_scan_dir_menu(index)?;
                if did_resolve {
                    statuses_dirty = true;
                }
            }
            TopEntry::ResolveAll => {
                let dirs = config.resolved_scan_dirs()?;
                for dir in &dirs {
                    if let Err(error) = resolve_duplicates(dir, false, true, false) {
                        FzfWrapper::message(&format!(
                            "Failed to resolve duplicates in {}: {}",
                            dir.display_path(),
                            error
                        ))?;
                    }
                    if let Err(error) = resolve_conflicts(dir, false) {
                        FzfWrapper::message(&format!(
                            "Failed to resolve conflicts in {}: {}",
                            dir.display_path(),
                            error
                        ))?;
                    }
                }
                statuses_dirty = true;
            }
            TopEntry::AddScanDir => {
                if add_scan_directory()? {
                    statuses_dirty = true;
                }
            }
            TopEntry::EditConfig => {
                edit_config()?;
            }
            TopEntry::Close => return Ok(()),
        }
    }
}

/// Build top-level menu entries using pre-computed statuses. Statuses are
/// indexed by position in `config.scan_dirs`; out-of-range indices fall back
/// to a placeholder "unavailable" status.
fn build_top_entries(config: &ResolvethingConfig, statuses: &[ScanDirStatus]) -> Vec<TopEntry> {
    let mut entries: Vec<TopEntry> = config
        .scan_dirs
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let resolved = config.resolved_scan_dir_or_fallback(index);
            let status = statuses.get(index).cloned().unwrap_or(ScanDirStatus {
                duplicate_count: None,
                conflict_count: None,
                warnings: vec!["Status unavailable".to_string()],
                exists: false,
            });
            TopEntry::ScanDir {
                index,
                resolved,
                status,
            }
        })
        .collect();

    if !config.scan_dirs.is_empty() {
        entries.push(TopEntry::ResolveAll);
    }
    entries.push(TopEntry::AddScanDir);
    entries.push(TopEntry::EditConfig);
    entries.push(TopEntry::Close);
    entries
}

/// Compute statuses for all configured scan directories up front so the menu
/// loop does not need to re-run fclones on every redraw.
fn compute_all_statuses(config: &ResolvethingConfig) -> Vec<ScanDirStatus> {
    (0..config.scan_dirs.len())
        .map(|i| {
            config
                .resolved_scan_dir(i)
                .map(|r| compute_status(&r))
                .unwrap_or(ScanDirStatus {
                    duplicate_count: None,
                    conflict_count: None,
                    warnings: vec!["Failed to resolve scan dir path".to_string()],
                    exists: false,
                })
        })
        .collect()
}

/// Returns `(ActionResult, did_resolve)` where `did_resolve` signals to the
/// outer menu that statuses should be invalidated.
fn run_scan_dir_menu(index: usize) -> Result<(ActionResult, bool)> {
    let mut cursor = MenuCursor::new();
    let mut cached_status: Option<ScanDirStatus> = None;
    let mut status_dirty = true;
    let mut did_resolve = false;

    loop {
        let config = resolved_config()?;
        if config.scan_dirs.get(index).is_none() {
            // Removed or shifted: bounce back to the outer menu.
            return Ok((ActionResult::Back, did_resolve));
        }
        let resolved = config.resolved_scan_dir_or_fallback(index);

        if status_dirty || cached_status.is_none() {
            cached_status = Some(compute_status(&resolved));
            status_dirty = false;
        }
        let status = cached_status.as_ref().unwrap();
        let actions = build_actions(&resolved, status);

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!(
                "Scan Dir: {}",
                resolved.display_path()
            )))
            .prompt("Action")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(idx) = cursor.initial_index(&actions) {
            builder = builder.initial_index(idx);
        }

        let selected_action = match builder.select(actions.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &actions);
                item.action
            }
            _ => ScanDirAction::Back,
        };

        // Mark statuses dirty before executing resolve actions so the next
        // render reflects the updated duplicate/conflict counts.
        let changes_status = matches!(
            selected_action,
            ScanDirAction::ResolveEverything
                | ScanDirAction::ResolveDuplicates
                | ScanDirAction::ResolveConflicts
        );

        let result = handle_scan_dir_action(selected_action, index, &resolved)?;

        if changes_status {
            status_dirty = true;
            did_resolve = true;
        }

        match result {
            ActionResult::Stay => continue,
            ActionResult::Back => return Ok((ActionResult::Stay, did_resolve)),
        }
    }
}

fn handle_scan_dir_action(
    action: ScanDirAction,
    index: usize,
    resolved: &ResolvedScanDir,
) -> Result<ActionResult> {
    match action {
        ScanDirAction::ResolveEverything => {
            if let Err(error) = resolve_duplicates(resolved, false, true, false) {
                FzfWrapper::message(&format!("Duplicate resolution failed: {}", error))?;
            }
            if let Err(error) = resolve_conflicts(resolved, false) {
                FzfWrapper::message(&format!("Conflict resolution failed: {}", error))?;
            }
            Ok(ActionResult::Stay)
        }
        ScanDirAction::ResolveDuplicates => {
            if let Err(error) = resolve_duplicates(resolved, false, true, false) {
                FzfWrapper::message(&format!("Duplicate resolution failed: {}", error))?;
            }
            Ok(ActionResult::Stay)
        }
        ScanDirAction::ResolveConflicts => {
            if let Err(error) = resolve_conflicts(resolved, false) {
                FzfWrapper::message(&format!("Conflict resolution failed: {}", error))?;
            }
            Ok(ActionResult::Stay)
        }
        ScanDirAction::OpenDirectory => {
            open_directory(&resolved.path)?;
            Ok(ActionResult::Stay)
        }
        ScanDirAction::EditExtensions => {
            configure_scan_directory_extensions(index)?;
            Ok(ActionResult::Stay)
        }
        ScanDirAction::ChangePath => {
            change_scan_directory_path(index)?;
            Ok(ActionResult::Stay)
        }
        ScanDirAction::Remove => {
            let confirm = FzfWrapper::confirm(&format!(
                "Remove {} from configured scan directories?",
                resolved.display_path()
            ))?;
            if matches!(confirm, ConfirmResult::Yes) {
                remove_scan_directory(index)?;
                Ok(ActionResult::Back)
            } else {
                Ok(ActionResult::Stay)
            }
        }
        ScanDirAction::Back => Ok(ActionResult::Back),
    }
}

fn build_actions(resolved: &ResolvedScanDir, status: &ScanDirStatus) -> Vec<ActionItem> {
    let entries = vec![
        ScanDirAction::ResolveEverything,
        ScanDirAction::ResolveDuplicates,
        ScanDirAction::ResolveConflicts,
        ScanDirAction::OpenDirectory,
        ScanDirAction::EditExtensions,
        ScanDirAction::ChangePath,
        ScanDirAction::Remove,
        ScanDirAction::Back,
    ];

    entries
        .into_iter()
        .map(|action| ActionItem {
            preview: build_action_preview(&action, resolved, status),
            action,
        })
        .collect()
}

fn compute_status(resolved: &ResolvedScanDir) -> ScanDirStatus {
    let mut warnings = Vec::new();

    if !resolved.path.exists() {
        warnings.push(format!(
            "Scan directory does not exist: {}",
            resolved.display_path()
        ));
        return ScanDirStatus {
            duplicate_count: None,
            conflict_count: None,
            warnings,
            exists: false,
        };
    }

    let duplicate_count = if which::which("fclones").is_ok() {
        match super::duplicates::scan_duplicates(&resolved.path) {
            Ok(groups) => {
                // Count only groups that require user action (Auto or Manual),
                // not Skip groups. Raw fclones output includes ignored-folder
                // and singleton groups that the planner will never act on, so
                // showing them as "duplicates" is misleading.
                let actionable = groups
                    .iter()
                    .filter(|g| !matches!(g.plan(false), GroupPlan::Skip(_)))
                    .count();
                Some(actionable)
            }
            Err(error) => {
                warnings.push(format!("Duplicate scan unavailable: {}", error));
                None
            }
        }
    } else {
        warnings.push("Duplicate scan unavailable: `fclones` not found in PATH".to_string());
        None
    };

    let conflict_count =
        match super::conflicts::scan_conflicts(&resolved.path, &resolved.extensions) {
            Ok(conflicts) => Some(conflicts.len()),
            Err(error) => {
                warnings.push(format!("Conflict scan unavailable: {}", error));
                None
            }
        };

    ScanDirStatus {
        duplicate_count,
        conflict_count,
        warnings,
        exists: true,
    }
}

fn build_top_preview(entry: &TopEntry, config: &ResolvethingConfig) -> String {
    match entry {
        TopEntry::ScanDir {
            resolved, status, ..
        } => {
            let dup = status
                .duplicate_count
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unavailable".to_string());
            let cnf = status
                .conflict_count
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unavailable".to_string());
            let mut builder = PreviewBuilder::new()
                .header(NerdFont::Folder, "Scan Directory")
                .field("Path", &resolved.display_path())
                .field("Extensions", &resolved.extensions_label())
                .field("Duplicate Groups", &dup)
                .field("Conflicts", &cnf);
            for warning in &status.warnings {
                builder = builder
                    .blank()
                    .line(colors::YELLOW, Some(NerdFont::Warning), warning);
            }
            builder.build_string()
        }
        TopEntry::ResolveAll => PreviewBuilder::new()
            .header(NerdFont::Sync, "Resolve All Scan Dirs")
            .text(&format!(
                "Resolve duplicates then conflicts for all {} configured directories.",
                config.scan_dirs.len()
            ))
            .build_string(),
        TopEntry::AddScanDir => PreviewBuilder::new()
            .header(NerdFont::Plus, "Add Scan Directory")
            .text("Add another directory to the resolvething scan list.")
            .blank()
            .text("New entries default to scanning every plain text file;")
            .text("you can pin them to specific extensions afterwards.")
            .build_string(),
        TopEntry::EditConfig => {
            let path = ResolvethingConfig::config_path()
                .map(|p| format_path(&p))
                .unwrap_or_else(|_| "<unavailable>".to_string());
            PreviewBuilder::new()
                .header(NerdFont::Edit, "Edit Config File")
                .field("File", &path)
                .blank()
                .text("Open the config file in your editor for direct edits.")
                .build_string()
        }
        TopEntry::Close => PreviewBuilder::new()
            .header(NerdFont::Cross, "Close Menu")
            .text("Exit the resolvething menu.")
            .build_string(),
    }
}

fn build_action_preview(
    action: &ScanDirAction,
    resolved: &ResolvedScanDir,
    status: &ScanDirStatus,
) -> String {
    let dup = status
        .duplicate_count
        .map(|c| c.to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let cnf = status
        .conflict_count
        .map(|c| c.to_string())
        .unwrap_or_else(|| "unavailable".to_string());

    let mut builder = match action {
        ScanDirAction::ResolveEverything => PreviewBuilder::new()
            .header(NerdFont::Sync, "Resolve Everything")
            .field("Path", &resolved.display_path())
            .field("Duplicate Groups", &dup)
            .field("Conflicts", &cnf)
            .blank()
            .text("Run duplicate cleanup first, then open conflict resolution."),
        ScanDirAction::ResolveDuplicates => PreviewBuilder::new()
            .header(NerdFont::File, "Resolve Duplicates")
            .field("Path", &resolved.display_path())
            .field("Duplicate Groups", &dup)
            .blank()
            .text("Conflict-file dupes are auto-cleaned (even in .stversions).")
            .text("Other duplicate groups prompt for which copy to keep."),
        ScanDirAction::ResolveConflicts => PreviewBuilder::new()
            .header(NerdFont::GitCompare, "Resolve Conflicts")
            .field("Path", &resolved.display_path())
            .field("Extensions", &resolved.extensions_label())
            .field("Conflicts", &cnf)
            .blank()
            .text("Open each conflict in your diff editor."),
        ScanDirAction::OpenDirectory => PreviewBuilder::new()
            .header(NerdFont::FolderOpen, "Open Directory")
            .field("Path", &resolved.display_path())
            .blank()
            .text("Try xdg-open, fall back to printing the path."),
        ScanDirAction::EditExtensions => PreviewBuilder::new()
            .header(NerdFont::Edit, "Edit Extensions")
            .field("Current", &resolved.extensions_label())
            .blank()
            .text("Comma-separated list of extensions to treat as conflicts.")
            .text("Empty = scan every plain text file."),
        ScanDirAction::ChangePath => PreviewBuilder::new()
            .header(NerdFont::Folder, "Change Path")
            .field("Current", &resolved.display_path())
            .blank()
            .text("Pick a new directory for this scan entry."),
        ScanDirAction::Remove => PreviewBuilder::new()
            .header(NerdFont::Trash, "Remove Scan Directory")
            .field("Path", &resolved.display_path())
            .blank()
            .text("Drop this entry from the configured scan list."),
        ScanDirAction::Back => PreviewBuilder::new()
            .header(NerdFont::Cross, "Back")
            .text("Return to the top resolvething menu."),
    };

    if !status.warnings.is_empty() {
        builder = builder.blank().separator().blank();
        for warning in &status.warnings {
            builder = builder.line(colors::YELLOW, Some(NerdFont::Warning), warning);
        }
    }

    builder.build_string()
}

fn open_directory(path: &Path) -> Result<()> {
    if which::which("xdg-open").is_ok() {
        let _ = Command::new("xdg-open").arg(path).spawn();
    } else {
        println!("{}", format_path(path));
    }
    Ok(())
}
