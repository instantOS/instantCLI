use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

use crate::menu_utils::{
    ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::commands::{
    add_scan_directory, change_scan_directory_path, configure_scan_directory_extensions,
    edit_config, remove_scan_directory, resolve_conflicts, resolve_duplicates, resolved_config,
};
use super::config::{ResolvedScanDir, ResolvethingConfig, format_path};

/// Outcome of an inner action: keep loop, leave to outer, or exit menu.
enum ActionResult {
    Stay,
    Back,
    Exit,
}

/// Top-level menu entry: each configured scan_dir is its own entry, plus a
/// few global actions modeled on `ins game menu`.
#[derive(Debug, Clone)]
enum TopEntry {
    ScanDir { index: usize, resolved: ResolvedScanDir, status: ScanDirStatus },
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
            TopEntry::ScanDir { resolved, status, .. } => {
                let dup = status
                    .duplicate_count
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let cnf = status
                    .conflict_count
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let icon_color = if status.exists { colors::TEAL } else { colors::RED };
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

    loop {
        let config = resolved_config()?;
        let entries = build_top_entries(&config);
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
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        };

        match selected {
            TopEntry::ScanDir { index, .. } => {
                if matches!(run_scan_dir_menu(index)?, ActionResult::Exit) {
                    return Ok(());
                }
            }
            TopEntry::ResolveAll => {
                let dirs = config.resolved_scan_dirs()?;
                for dir in &dirs {
                    if let Err(error) = resolve_duplicates(dir, false, true) {
                        FzfWrapper::message(&format!(
                            "Failed to resolve duplicates in {}: {}",
                            dir.display_path(),
                            error
                        ))?;
                    }
                    if let Err(error) = resolve_conflicts(dir) {
                        FzfWrapper::message(&format!(
                            "Failed to resolve conflicts in {}: {}",
                            dir.display_path(),
                            error
                        ))?;
                    }
                }
            }
            TopEntry::AddScanDir => {
                add_scan_directory()?;
            }
            TopEntry::EditConfig => {
                edit_config()?;
            }
            TopEntry::Close => return Ok(()),
        }
    }
}

fn build_top_entries(config: &ResolvethingConfig) -> Vec<TopEntry> {
    let mut entries: Vec<TopEntry> = config
        .scan_dirs
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let resolved = config
                .resolved_scan_dir(index)
                .unwrap_or_else(|_| ResolvedScanDir {
                    path: config.scan_dirs[index].path.clone(),
                    extensions: config.scan_dirs[index].normalized_extensions(),
                    source_index: Some(index),
                });
            let status = compute_status(&resolved);
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

fn run_scan_dir_menu(index: usize) -> Result<ActionResult> {
    let mut cursor = MenuCursor::new();
    loop {
        let config = resolved_config()?;
        let Some(entry) = config.scan_dirs.get(index).cloned() else {
            // Removed or shifted: bounce back to the outer menu.
            return Ok(ActionResult::Back);
        };
        let resolved = config
            .resolved_scan_dir(index)
            .unwrap_or_else(|_| ResolvedScanDir {
                path: entry.path.clone(),
                extensions: entry.normalized_extensions(),
                source_index: Some(index),
            });
        let status = compute_status(&resolved);
        let actions = build_actions(&resolved, &status);

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

        let result = match builder.select(actions.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &actions);
                handle_scan_dir_action(item.action, index, &resolved)?
            }
            FzfResult::Cancelled => ActionResult::Back,
            _ => ActionResult::Exit,
        };

        match result {
            ActionResult::Stay => continue,
            ActionResult::Back => return Ok(ActionResult::Stay),
            ActionResult::Exit => return Ok(ActionResult::Exit),
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
            if let Err(error) = resolve_duplicates(resolved, false, true) {
                FzfWrapper::message(&format!("Duplicate resolution failed: {}", error))?;
            }
            if let Err(error) = resolve_conflicts(resolved) {
                FzfWrapper::message(&format!("Conflict resolution failed: {}", error))?;
            }
            Ok(ActionResult::Stay)
        }
        ScanDirAction::ResolveDuplicates => {
            if let Err(error) = resolve_duplicates(resolved, false, true) {
                FzfWrapper::message(&format!("Duplicate resolution failed: {}", error))?;
            }
            Ok(ActionResult::Stay)
        }
        ScanDirAction::ResolveConflicts => {
            if let Err(error) = resolve_conflicts(resolved) {
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
            Ok(groups) => Some(groups.len()),
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
        TopEntry::ScanDir { resolved, status, .. } => {
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

fn open_directory(path: &PathBuf) -> Result<()> {
    if which::which("xdg-open").is_ok() {
        let _ = Command::new("xdg-open").arg(path).spawn();
    } else {
        println!("{}", format_path(path));
    }
    Ok(())
}
