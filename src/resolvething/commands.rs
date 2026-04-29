use anyhow::{Context, Result, bail};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::deps::FZF;
use crate::common::package::{Dependency, InstallResult, PackageDefinition, PackageManager, ensure_all};
use crate::common::requirements::InstallTest;
use crate::menu_utils::{FzfResult, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::{Level, emit};

use super::cli::{ConfigCommands, ResolvethingCommands};
use super::config::{ResolvethingConfig, format_path};
use super::conflicts::{ConflictChoice, ConflictResolution, scan_conflicts};
use super::duplicates::{DuplicateChoice, DuplicateGroup, scan_duplicates};
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
        ResolvethingCommands::Duplicates { path, no_auto } => resolve_duplicates(path.as_deref(), no_auto),
        ResolvethingCommands::Conflicts { path, types } => resolve_conflicts(path.as_deref(), &types),
        ResolvethingCommands::All {
            path,
            no_auto,
            types,
        } => {
            resolve_duplicates(path.as_deref(), no_auto)?;
            resolve_conflicts(path.as_deref(), &types)
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
            ConfigCommands::Path => {
                println!("{}", show_config_path()?);
                Ok(())
            }
            ConfigCommands::Show => show_config(),
            ConfigCommands::Edit => edit_config(),
        },
    }
}

pub fn resolve_duplicates(path_override: Option<&str>, no_auto: bool) -> Result<()> {
    ensure_duplicate_dependencies()?;
    let config = resolved_config()?;
    let directory = config.resolve_working_directory(path_override)?;

    if !directory.exists() {
        bail!("Working directory does not exist: {}", directory.display());
    }

    let groups = scan_duplicates(&directory)?;
    if groups.is_empty() {
        emit(
            Level::Success,
            "resolvething.duplicates.none",
            &format!(
                "{} No duplicate groups found in {}",
                char::from(NerdFont::Check),
                format_path(&directory)
            ),
            None,
        );
        return Ok(());
    }

    let mut removed_files = 0usize;
    let mut auto_resolved = 0usize;
    let mut skipped = 0usize;
    let mut cursor = MenuCursor::new();

    for (index, group) in groups.iter().enumerate() {
        let keep = if !no_auto {
            group.auto_keep_choice().map(|entry| entry.path.clone())
        } else {
            None
        };

        let keep = match keep {
            Some(path) => {
                auto_resolved += 1;
                emit(
                    Level::Info,
                    "resolvething.duplicates.auto",
                    &format!(
                        "{} Auto-keeping {}",
                        char::from(NerdFont::Check),
                        format_path(&path)
                    ),
                    None,
                );
                Some(path)
            }
            None => select_duplicate_keep(group, index + 1, groups.len(), &mut cursor)?,
        };

        if let Some(keep_path) = keep {
            removed_files += group.keep_only(&keep_path)?;
        } else {
            skipped += 1;
        }
    }

    emit(
        Level::Success,
        "resolvething.duplicates.complete",
        &format!(
            "{} Duplicate cleanup finished: {} groups, {} auto-resolved, {} skipped, {} files trashed",
            char::from(NerdFont::Check),
            groups.len(),
            auto_resolved,
            skipped,
            removed_files
        ),
        None,
    );

    Ok(())
}

pub fn resolve_conflicts(path_override: Option<&str>, type_overrides: &[String]) -> Result<()> {
    ensure_menu_dependencies()?;
    let config = resolved_config()?;
    let directory = config.resolve_working_directory(path_override)?;
    let types = config.normalized_conflict_types(type_overrides);

    if !directory.exists() {
        bail!("Working directory does not exist: {}", directory.display());
    }
    if types.is_empty() {
        bail!("No conflict file types configured");
    }

    let mut conflicts = scan_conflicts(&directory, &types)?;
    conflicts.retain(|conflict| conflict.is_valid());

    if conflicts.is_empty() {
        emit(
            Level::Success,
            "resolvething.conflicts.none",
            &format!(
                "{} No resolvable Syncthing conflicts found in {}",
                char::from(NerdFont::Check),
                format_path(&directory)
            ),
            None,
        );
        return Ok(());
    }

    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    let mut skipped = 0usize;
    let mut cursor = MenuCursor::new();

    loop {
        conflicts = scan_conflicts(&directory, &types)?;
        conflicts.retain(|conflict| conflict.is_valid());
        if conflicts.is_empty() {
            break;
        }

        let choice = select_conflict_choice(&conflicts, &mut cursor)?;
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
            "{} Conflict cleanup finished: {} resolved, {} unresolved, {} skipped",
            char::from(NerdFont::Check),
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

pub fn show_config_path() -> Result<String> {
    Ok(ResolvethingConfig::config_path()?.display().to_string())
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

pub fn sync_conflict_regex() -> Regex {
    Regex::new(r".*\.sync-conflict-[A-Z0-9-]*(\..*)?$")
        .expect("invalid Syncthing conflict regex")
}

pub fn sync_conflict_regex_for_type(file_type: &str) -> Regex {
    Regex::new(&format!(r".*\.sync-conflict-[A-Z0-9-]*\.{}$", regex::escape(file_type)))
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
        let status = Command::new("gio").arg("trash").arg(path).status()?;
        if status.success() {
            return Ok(());
        }
    }

    bail!(
        "Unable to move {} to trash. Install `trash` or ensure `gio` is available.",
        path.display()
    )
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

    let parts = shell_words::split(&raw).with_context(|| format!("parsing editor command '{raw}'"))?;
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
    cursor: &mut MenuCursor,
) -> Result<Option<ConflictChoice>> {
    let mut entries = Vec::with_capacity(conflicts.len() + 2);
    entries.push(ConflictChoice::ResolveAll);
    entries.extend(conflicts.iter().cloned().map(ConflictChoice::Resolve));
    entries.push(ConflictChoice::Close);

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Syncthing Conflicts"))
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
