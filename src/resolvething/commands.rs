use anyhow::{Context, Result, bail};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::deps::FZF;
use crate::common::package::{
    Dependency, InstallResult, PackageDefinition, PackageManager, ensure_all,
};
use crate::common::requirements::InstallTest;
use crate::menu_utils::{
    FilePickerScope, FzfResult, FzfWrapper, Header, MenuCursor, PathInputBuilder,
    PathInputSelection, TextEditOutcome, TextEditPrompt, prompt_text_edit,
};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::{Level, emit};

use super::cli::{ConfigCommands, ResolvethingCommands};
use super::config::{ResolvedScanDir, ResolvethingConfig, ScanDir, format_path};
use super::conflicts::{ConflictChoice, ConflictResolution, scan_conflicts};
use super::duplicates::{DuplicateChoice, DuplicateGroup, GroupPlan, SkipReason, scan_duplicates};
use super::menu::resolvething_menu;

static FCLONES_DEP: Dependency = Dependency {
    name: "fclones",
    packages: &[
        PackageDefinition::new("fclones", PackageManager::Pacman),
        PackageDefinition::new("fclones", PackageManager::Cargo),
    ],
    tests: &[InstallTest::WhichSucceeds("fclones")],
};

pub fn handle_resolvething_command(command: ResolvethingCommands, debug: bool) -> Result<()> {
    match command {
        ResolvethingCommands::Duplicates {
            dir,
            no_auto,
            show_ignored,
            dry_run,
        } => {
            let scan_dirs = resolve_scan_dirs(dir.as_deref())?;
            for scan_dir in &scan_dirs {
                resolve_duplicates(scan_dir, no_auto, show_ignored, dry_run)?;
            }
            Ok(())
        }
        ResolvethingCommands::Conflicts { dir, dry_run } => {
            let scan_dirs = resolve_scan_dirs(dir.as_deref())?;
            for scan_dir in &scan_dirs {
                resolve_conflicts(scan_dir, dry_run)?;
            }
            Ok(())
        }
        ResolvethingCommands::All {
            dir,
            no_auto,
            show_ignored,
            dry_run,
        } => {
            let scan_dirs = resolve_scan_dirs(dir.as_deref())?;
            for scan_dir in &scan_dirs {
                resolve_duplicates(scan_dir, no_auto, show_ignored, dry_run)?;
                resolve_conflicts(scan_dir, dry_run)?;
            }
            Ok(())
        }
        ResolvethingCommands::Menu { gui } => {
            if gui {
                return crate::common::terminal::launch_menu_in_terminal(
                    "resolvething",
                    "Resolvething",
                    &[],
                    debug,
                );
            }
            ensure_menu_dependencies()?;
            resolvething_menu(debug)
        }
        ResolvethingCommands::Config { command } => match command.unwrap_or(ConfigCommands::Show) {
            ConfigCommands::Show => show_config(),
            ConfigCommands::Edit => edit_config(),
        },
    }
}

/// Resolve which scan dirs to operate on for a CLI invocation.
pub fn resolve_scan_dirs(dir_override: Option<&str>) -> Result<Vec<ResolvedScanDir>> {
    let config = resolved_config()?;
    if let Some(raw) = dir_override {
        let resolved = config.resolved_scan_dir_for_override(raw)?;
        return Ok(vec![resolved]);
    }
    let resolved = config.resolved_scan_dirs()?;
    if resolved.is_empty() {
        bail!(
            "No scan directories configured. Add one with `ins resolvething menu` or edit {}",
            ResolvethingConfig::config_path()?.display()
        );
    }
    Ok(resolved)
}

pub fn resolve_duplicates(
    scan_dir: &ResolvedScanDir,
    no_auto: bool,
    show_ignored: bool,
    dry_run: bool,
) -> Result<()> {
    ensure_duplicate_dependencies()?;

    if !scan_dir.path.exists() {
        bail!("Scan directory does not exist: {}", scan_dir.path.display());
    }

    let groups = scan_duplicates(&scan_dir.path)?;
    if groups.is_empty() {
        emit(
            Level::Success,
            "resolvething.duplicates.none",
            &format!(
                "{} No duplicate groups found in {}",
                char::from(NerdFont::Check),
                format_path(&scan_dir.path)
            ),
            None,
        );
        return Ok(());
    }

    let mut removed_files = 0usize;
    let mut auto_resolved = 0usize;
    let mut skipped = 0usize;
    let mut ignored_groups: Vec<&DuplicateGroup> = Vec::new();
    let mut cursor = MenuCursor::new();

    for (index, group) in groups.iter().enumerate() {
        match group.plan(no_auto) {
            GroupPlan::Auto(action) => {
                auto_resolved += 1;
                for path in &action.keep {
                    emit(
                        Level::Info,
                        "resolvething.duplicates.auto.keep",
                        &format!(
                            "{} Keeping {}",
                            char::from(NerdFont::Check),
                            format_path(path)
                        ),
                        None,
                    );
                }
                for path in &action.trash {
                    emit(
                        Level::Info,
                        "resolvething.duplicates.auto",
                        &format!(
                            "{} {} duplicate {}",
                            char::from(NerdFont::Trash),
                            if dry_run { "Would trash" } else { "Trashing" },
                            format_path(path)
                        ),
                        None,
                    );
                }
                if !dry_run {
                    removed_files += group.keep_paths(&action.keep)?;
                } else {
                    removed_files += action.trash.len();
                }
            }
            GroupPlan::Manual => {
                if dry_run {
                    emit(
                        Level::Info,
                        "resolvething.duplicates.manual.dry_run",
                        &format!(
                            "{} Would prompt for manual selection ({} files)",
                            char::from(NerdFont::Info),
                            group.files.len()
                        ),
                        None,
                    );
                    skipped += 1;
                } else {
                    let keep = select_duplicate_keep(group, index + 1, groups.len(), &mut cursor)?;
                    if let Some(keep_path) = keep {
                        removed_files += group.keep_paths(&[keep_path])?;
                    } else {
                        skipped += 1;
                    }
                }
            }
            GroupPlan::Skip(SkipReason::IgnoredFolder) => {
                ignored_groups.push(group);
            }
        }
    }

    if show_ignored && !ignored_groups.is_empty() {
        emit(
            Level::Info,
            "resolvething.duplicates.ignored.header",
            &format!(
                "{} {} ignored duplicate group(s) in {}:",
                char::from(NerdFont::Info),
                ignored_groups.len(),
                format_path(&scan_dir.path)
            ),
            None,
        );
        for (i, group) in ignored_groups.iter().enumerate() {
            emit(
                Level::Info,
                "resolvething.duplicates.ignored.group",
                &format!("  Group {}:", i + 1),
                None,
            );
            for entry in &group.files {
                emit(
                    Level::Info,
                    "resolvething.duplicates.ignored.file",
                    &format!("    - {}", format_path(&entry.path)),
                    None,
                );
            }
        }
    }

    let ignored_hint = if !show_ignored && !ignored_groups.is_empty() {
        " (re-run with --show-ignored to list them)"
    } else {
        ""
    };

    let dry_run_suffix = if dry_run { " [dry run]" } else { "" };
    emit(
        Level::Success,
        "resolvething.duplicates.complete",
        &format!(
            "{} {}{}: {} groups, {} auto-resolved, {} skipped, {} ignored{}, {} files {}",
            char::from(NerdFont::Check),
            format_path(&scan_dir.path),
            dry_run_suffix,
            groups.len(),
            auto_resolved,
            skipped,
            ignored_groups.len(),
            ignored_hint,
            removed_files,
            if dry_run {
                "would be trashed"
            } else {
                "trashed"
            },
        ),
        None,
    );

    Ok(())
}

pub fn resolve_conflicts(scan_dir: &ResolvedScanDir, dry_run: bool) -> Result<()> {
    let config = if dry_run {
        // Config is only needed for the editor command; skip fzf dep check in dry-run mode
        resolved_config()?
    } else {
        ensure_menu_dependencies()?;
        resolved_config()?
    };

    if !scan_dir.path.exists() {
        bail!("Scan directory does not exist: {}", scan_dir.path.display());
    }

    let mut conflicts = scan_conflicts(&scan_dir.path, &scan_dir.extensions)?;
    conflicts.retain(|conflict| conflict.is_valid());

    if conflicts.is_empty() {
        emit(
            Level::Success,
            "resolvething.conflicts.none",
            &format!(
                "{} No resolvable Syncthing conflicts found in {}",
                char::from(NerdFont::Check),
                format_path(&scan_dir.path)
            ),
            None,
        );
        return Ok(());
    }

    if dry_run {
        emit(
            Level::Info,
            "resolvething.conflicts.dry_run",
            &format!(
                "{} {} conflict(s) would need resolution in {} [dry run]:",
                char::from(NerdFont::Info),
                conflicts.len(),
                format_path(&scan_dir.path)
            ),
            None,
        );
        for conflict in &conflicts {
            emit(
                Level::Info,
                "resolvething.conflicts.dry_run.item",
                &format!(
                    "  {} {}  vs  {}",
                    char::from(NerdFont::GitCompare),
                    format_path(&conflict.modified),
                    format_path(&conflict.original),
                ),
                None,
            );
        }
        return Ok(());
    }

    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    let mut skipped = 0usize;
    let mut cursor = MenuCursor::new();

    loop {
        conflicts = scan_conflicts(&scan_dir.path, &scan_dir.extensions)?;
        conflicts.retain(|conflict| conflict.is_valid());
        if conflicts.is_empty() {
            break;
        }

        let choice = select_conflict_choice(&conflicts, &scan_dir.path, &mut cursor)?;
        let Some(choice) = choice else {
            break;
        };

        match choice {
            ConflictChoice::Resolve(conflict) => {
                match conflict.resolve(config.editor_command.as_deref())? {
                    ConflictResolution::Resolved => resolved += 1,
                    ConflictResolution::Unresolved => unresolved += 1,
                    ConflictResolution::SkippedInvalid => skipped += 1,
                }
            }
            ConflictChoice::ResolveAll => {
                for conflict in conflicts.clone() {
                    match conflict.resolve(config.editor_command.as_deref())? {
                        ConflictResolution::Resolved => resolved += 1,
                        ConflictResolution::Unresolved => unresolved += 1,
                        ConflictResolution::SkippedInvalid => skipped += 1,
                    }
                }
            }
            ConflictChoice::Close => break,
        }
    }

    emit(
        Level::Success,
        "resolvething.conflicts.complete",
        &format!(
            "{} {}: {} resolved, {} unresolved, {} skipped",
            char::from(NerdFont::Check),
            format_path(&scan_dir.path),
            resolved,
            unresolved,
            skipped
        ),
        None,
    );

    Ok(())
}

pub fn resolved_config() -> Result<ResolvethingConfig> {
    let config = ResolvethingConfig::load()?;
    if !ResolvethingConfig::config_path()?.exists() {
        config.save()?;
    }
    Ok(config)
}

pub fn show_config() -> Result<()> {
    let config_path = ResolvethingConfig::config_path()?;
    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    print!("{contents}");
    Ok(())
}

pub fn edit_config() -> Result<()> {
    let path = ResolvethingConfig::config_path()?;
    let config = resolved_config()?;
    let mut command = plain_editor_command(config.editor_command.as_deref())?;
    command.arg(&path);
    command
        .status()
        .with_context(|| format!("launching editor for {}", path.display()))?;
    Ok(())
}

pub fn add_scan_directory() -> Result<bool> {
    let mut config = resolved_config()?;

    let mut builder = PathInputBuilder::new()
        .header(format!(
            "{} Add a scan directory",
            char::from(NerdFont::Folder)
        ))
        .manual_prompt(format!(
            "{} Enter the directory to scan",
            char::from(NerdFont::Edit)
        ))
        .scope(FilePickerScope::Directories)
        .picker_hint(format!(
            "{} Pick a directory Syncthing writes into",
            char::from(NerdFont::Info)
        ))
        .manual_option_label(format!(
            "{} Type an exact directory",
            char::from(NerdFont::Edit)
        ))
        .picker_option_label(format!(
            "{} Browse and choose a directory",
            char::from(NerdFont::FolderOpen)
        ));

    if let Some(home) = dirs::home_dir() {
        builder = builder.start_dir(home);
    }

    let selection = builder.choose()?;
    let chosen = match selection {
        PathInputSelection::Manual(input) => super::config::expand_path(&input)?,
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => path,
        PathInputSelection::Cancelled => return Ok(false),
    };

    if config.scan_dirs.iter().any(|entry| entry.path == chosen) {
        FzfWrapper::message(&format!(
            "{} is already configured as a scan directory.",
            format_path(&chosen)
        ))?;
        return Ok(false);
    }

    config.scan_dirs.push(ScanDir::new(chosen.clone()));
    config.save()?;

    emit(
        Level::Success,
        "resolvething.config.scan_dir_added",
        &format!(
            "{} Added scan directory {}",
            char::from(NerdFont::Check),
            format_path(&chosen)
        ),
        None,
    );

    Ok(true)
}

pub fn remove_scan_directory(index: usize) -> Result<bool> {
    let mut config = resolved_config()?;
    if index >= config.scan_dirs.len() {
        return Ok(false);
    }
    let removed = config.scan_dirs.remove(index);
    config.save()?;
    emit(
        Level::Success,
        "resolvething.config.scan_dir_removed",
        &format!(
            "{} Removed scan directory {}",
            char::from(NerdFont::Check),
            format_path(&removed.path)
        ),
        None,
    );
    Ok(true)
}

pub fn change_scan_directory_path(index: usize) -> Result<bool> {
    let mut config = resolved_config()?;
    let current = config
        .scan_dirs
        .get(index)
        .map(|entry| entry.path.clone())
        .ok_or_else(|| anyhow::anyhow!("scan_dir index {} out of range", index))?;

    let builder = PathInputBuilder::new()
        .header(format!(
            "{} Change scan directory path",
            char::from(NerdFont::Folder)
        ))
        .manual_prompt(format!(
            "{} Enter the new directory",
            char::from(NerdFont::Edit)
        ))
        .scope(FilePickerScope::Directories)
        .start_dir(current.clone())
        .manual_option_label(format!(
            "{} Type an exact directory",
            char::from(NerdFont::Edit)
        ))
        .picker_option_label(format!(
            "{} Browse and choose a directory",
            char::from(NerdFont::FolderOpen)
        ));

    let selection = builder.choose()?;
    let chosen = match selection {
        PathInputSelection::Manual(input) => super::config::expand_path(&input)?,
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => path,
        PathInputSelection::Cancelled => return Ok(false),
    };

    config.scan_dirs[index].path = chosen.clone();
    config.save()?;

    emit(
        Level::Success,
        "resolvething.config.scan_dir_path",
        &format!(
            "{} Scan directory updated to {}",
            char::from(NerdFont::Check),
            format_path(&chosen)
        ),
        None,
    );

    Ok(true)
}

pub fn configure_scan_directory_extensions(index: usize) -> Result<bool> {
    let mut config = resolved_config()?;
    let entry = config
        .scan_dirs
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("scan_dir index {} out of range", index))?;

    let current = if entry.extensions.is_empty() {
        None
    } else {
        Some(entry.extensions.join(", "))
    };

    let outcome = prompt_text_edit(
        TextEditPrompt::new("Conflict Extensions", current.as_deref())
            .header(format!(
                "Conflict file extensions for {}\nLeave empty to scan every plain text file.",
                format_path(&entry.path)
            ))
            .ghost("e.g. md,json,txt"),
    )?;

    match outcome {
        TextEditOutcome::Updated(value) => {
            let raw: Vec<String> = value
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(|item| item.to_string())
                .collect();
            let parsed = super::config::normalize_extensions(&raw);
            config.scan_dirs[index].extensions = parsed.clone();
            config.save()?;
            let label = if parsed.is_empty() {
                "all plain text files".to_string()
            } else {
                parsed.join(", ")
            };
            emit(
                Level::Success,
                "resolvething.config.scan_dir_extensions",
                &format!(
                    "{} Conflict extensions for {} set to {}",
                    char::from(NerdFont::Check),
                    format_path(&config.scan_dirs[index].path),
                    label
                ),
                None,
            );
            Ok(true)
        }
        TextEditOutcome::Unchanged | TextEditOutcome::Cancelled => Ok(false),
    }
}

pub fn sync_conflict_regex() -> Regex {
    Regex::new(r".*\.sync-conflict-[A-Z0-9-]*(\..*)?$").expect("invalid Syncthing conflict regex")
}

pub fn sync_conflict_replace_regex() -> Regex {
    Regex::new(r"\.sync-conflict-[A-Z0-9-]*").expect("invalid Syncthing conflict replacement regex")
}

pub fn sync_conflict_regex_for_type(file_type: &str) -> Regex {
    Regex::new(&format!(
        r".*\.sync-conflict-[A-Z0-9-]*\.{}$",
        regex::escape(file_type)
    ))
    .expect("invalid Syncthing conflict regex for type")
}

pub fn sync_conflict_replace_regex_for_type(file_type: &str) -> Regex {
    Regex::new(&format!(
        r"\.sync-conflict-[A-Z0-9-]*\.{}$",
        regex::escape(file_type)
    ))
    .expect("invalid Syncthing conflict replacement regex")
}

pub fn trash_path(path: &Path) -> Result<()> {
    if which::which("trash").is_ok() {
        let status = Command::new("trash").arg(path).status()?;
        if status.success() {
            return Ok(());
        }
    }

    if which::which("gio").is_ok() {
        let output = Command::new("gio").arg("trash").arg(path).output()?;
        if output.status.success() {
            return Ok(());
        }
        // On Termux/Android, gio refuses with
        //   "Trashing on system internal mounts is not supported"
        // because Android storage isn't a freedesktop-compatible mount. Fall
        // through to the manual XDG trash implementation in that case.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("system internal mount") {
            eprintln!("gio trash failed: {}", stderr.trim());
        }
    }

    manual_trash(path).with_context(|| {
        format!(
            "Unable to move {} to trash. Install `trash` or ensure `gio` is available.",
            path.display()
        )
    })
}

/// Move `path` into `$XDG_DATA_HOME/Trash/files`, creating the directory if
/// needed. This is a minimal fallback used when neither `trash` nor `gio`
/// can handle the move (e.g. on Termux, where Android storage is rejected by
/// gio as a "system internal mount").
fn manual_trash(path: &Path) -> Result<()> {
    let trash_dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Trash")
        .join("files");
    std::fs::create_dir_all(&trash_dir).with_context(|| {
        format!(
            "Failed to create fallback trash directory at {}",
            trash_dir.display()
        )
    })?;

    let file_name = path.file_name().ok_or_else(|| {
        anyhow::anyhow!("Cannot trash path without a file name: {}", path.display())
    })?;
    let mut target = trash_dir.join(file_name);
    if target.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        target = trash_dir.join(format!("{}.{}", file_name.to_string_lossy(), ts));
    }

    if std::fs::rename(path, &target).is_ok() {
        return Ok(());
    }

    // Fallback for cross-filesystem moves (e.g. between /sdcard and Termux
    // home): copy then delete. Directories are not supported via copy because
    // we don't want to pull in a recursive-copy dependency for an edge case.
    if path.is_dir() {
        bail!(
            "Cannot trash directory {} across filesystems; install `trash` or remove it manually",
            path.display()
        );
    }
    std::fs::copy(path, &target).with_context(|| {
        format!(
            "Failed to copy {} into fallback trash at {}",
            path.display(),
            target.display()
        )
    })?;
    std::fs::remove_file(path)
        .with_context(|| format!("Failed to remove {} after copying to trash", path.display()))?;
    Ok(())
}

pub fn editor_command(configured_editor: Option<&str>) -> Result<Command> {
    let mut command = plain_editor_command(configured_editor)?;
    command.arg("-d");
    Ok(command)
}

fn plain_editor_command(configured_editor: Option<&str>) -> Result<Command> {
    let raw = configured_editor
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "nvim".to_string());

    let parts =
        shell_words::split(&raw).with_context(|| format!("parsing editor command '{raw}'"))?;
    let Some((program, args)) = parts.split_first() else {
        bail!("Editor command is empty");
    };

    let mut command = Command::new(program);
    command.args(args);
    Ok(command)
}

fn ensure_duplicate_dependencies() -> Result<()> {
    match ensure_all(&[&FZF, &FCLONES_DEP])? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(()),
        InstallResult::Declined => bail!("dependency installation cancelled"),
        InstallResult::NotAvailable { hint, .. } => bail!("missing dependency: {hint}"),
        InstallResult::Failed { reason } => bail!("dependency installation failed: {reason}"),
    }
}

fn ensure_menu_dependencies() -> Result<()> {
    match ensure_all(&[&FZF])? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(()),
        InstallResult::Declined => bail!("fzf installation cancelled"),
        InstallResult::NotAvailable { hint, .. } => bail!("missing dependency: {hint}"),
        InstallResult::Failed { reason } => bail!("dependency installation failed: {reason}"),
    }
}

fn select_duplicate_keep(
    group: &DuplicateGroup,
    index: usize,
    total: usize,
    cursor: &mut MenuCursor,
) -> Result<Option<PathBuf>> {
    let mut entries: Vec<DuplicateChoice> = group
        .files
        .iter()
        .cloned()
        .map(DuplicateChoice::Keep)
        .collect();
    entries.push(DuplicateChoice::Skip);

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy(&format!("Duplicate Group {index}/{total}")))
        .prompt("Keep")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(initial_index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(initial_index);
    }

    match builder.select(entries.clone())? {
        FzfResult::Selected(choice) => {
            cursor.update(&choice, &entries);
            match choice {
                DuplicateChoice::Keep(file) => Ok(Some(file.path)),
                DuplicateChoice::Skip => Ok(None),
            }
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

fn select_conflict_choice(
    conflicts: &[super::conflicts::Conflict],
    scan_dir: &Path,
    cursor: &mut MenuCursor,
) -> Result<Option<ConflictChoice>> {
    let mut entries = Vec::with_capacity(conflicts.len() + 2);
    entries.push(ConflictChoice::ResolveAll);
    entries.extend(conflicts.iter().cloned().map(ConflictChoice::Resolve));
    entries.push(ConflictChoice::Close);

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy(&format!(
            "Syncthing Conflicts: {}",
            format_path(scan_dir)
        )))
        .prompt("Resolve")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(index);
    }

    match builder.select(entries.clone())? {
        FzfResult::Selected(choice) => {
            cursor.update(&choice, &entries);
            Ok(Some(choice))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}
