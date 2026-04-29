use anyhow::Result;
use std::process::Command;

use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::commands::{
    configure_conflict_types, configure_working_directory, resolve_conflicts, resolve_duplicates,
    resolved_config, show_config_path,
};
use super::config::{ResolvethingConfig, format_path};

#[derive(Debug, Clone)]
struct MenuStatus {
    config: ResolvethingConfig,
    directory: std::path::PathBuf,
    duplicate_count: Option<usize>,
    conflict_count: Option<usize>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
enum MenuEntry {
    ResolveAll,
    ResolveDuplicates,
    ResolveConflicts,
    ConfigureWorkingDirectory,
    ConfigureConflictTypes,
    EditConfig,
    OpenWorkingDirectory,
    ShowConfigPath,
    Close,
}

#[derive(Clone)]
struct MenuItem {
    entry: MenuEntry,
    preview: String,
}

impl FzfSelectable for MenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::ResolveAll => format!(
                "{} Resolve Everything",
                format_icon_colored(NerdFont::Sync, colors::GREEN)
            ),
            Self::ResolveDuplicates => format!(
                "{} Resolve Duplicates",
                format_icon_colored(NerdFont::File, colors::MAUVE)
            ),
            Self::ResolveConflicts => format!(
                "{} Resolve Conflicts",
                format_icon_colored(NerdFont::GitCompare, colors::PEACH)
            ),
            Self::ConfigureWorkingDirectory => format!(
                "{} Set Working Directory",
                format_icon_colored(NerdFont::Folder, colors::TEAL)
            ),
            Self::ConfigureConflictTypes => format!(
                "{} Set Conflict Types",
                format_icon_colored(NerdFont::Edit, colors::YELLOW)
            ),
            Self::EditConfig => format!(
                "{} Edit Config",
                format_icon_colored(NerdFont::Edit, colors::BLUE)
            ),
            Self::OpenWorkingDirectory => format!(
                "{} Open Working Directory",
                format_icon_colored(NerdFont::FolderOpen, colors::TEAL)
            ),
            Self::ShowConfigPath => format!(
                "{} Show Config Path",
                format_icon_colored(NerdFont::FileConfig, colors::YELLOW)
            ),
            Self::Close => format!("{} Close Menu", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::ResolveAll => "!__all__".to_string(),
            Self::ResolveDuplicates => "!__duplicates__".to_string(),
            Self::ResolveConflicts => "!__conflicts__".to_string(),
            Self::ConfigureWorkingDirectory => "!__configure_working_directory__".to_string(),
            Self::ConfigureConflictTypes => "!__configure_conflict_types__".to_string(),
            Self::EditConfig => "!__edit_config__".to_string(),
            Self::OpenWorkingDirectory => "!__open_dir__".to_string(),
            Self::ShowConfigPath => "!__config_path__".to_string(),
            Self::Close => "!__close__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Info, "Resolvething")
            .text("Preview is prepared when the menu opens.")
            .build()
    }
}

impl FzfSelectable for MenuItem {
    fn fzf_display_text(&self) -> String {
        self.entry.fzf_display_text()
    }

    fn fzf_key(&self) -> String {
        self.entry.fzf_key()
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(self.preview.clone())
    }
}

pub fn resolvething_menu(debug: bool) -> Result<()> {
    let _ = debug;
    let mut cursor = MenuCursor::new();

    loop {
        let selected = match select_menu_entry(&mut cursor)? {
            Some(entry) => entry,
            None => return Ok(()),
        };

        match selected {
            MenuEntry::ResolveAll => {
                resolve_duplicates(None, false)?;
                resolve_conflicts(None, &[])?;
            }
            MenuEntry::ResolveDuplicates => {
                resolve_duplicates(None, false)?;
            }
            MenuEntry::ResolveConflicts => {
                resolve_conflicts(None, &[])?;
            }
            MenuEntry::ConfigureWorkingDirectory => {
                configure_working_directory()?;
            }
            MenuEntry::ConfigureConflictTypes => {
                configure_conflict_types()?;
            }
            MenuEntry::EditConfig => {
                super::commands::edit_config()?;
            }
            MenuEntry::OpenWorkingDirectory => {
                open_working_directory()?;
            }
            MenuEntry::ShowConfigPath => {
                crate::menu_utils::FzfWrapper::message(&show_config_path()?)?;
            }
            MenuEntry::Close => return Ok(()),
        }
    }
}

fn select_menu_entry(cursor: &mut MenuCursor) -> Result<Option<MenuEntry>> {
    let entries = vec![
        MenuEntry::ResolveAll,
        MenuEntry::ResolveDuplicates,
        MenuEntry::ResolveConflicts,
        MenuEntry::ConfigureWorkingDirectory,
        MenuEntry::ConfigureConflictTypes,
        MenuEntry::EditConfig,
        MenuEntry::OpenWorkingDirectory,
        MenuEntry::ShowConfigPath,
        MenuEntry::Close,
    ];

    let status = menu_status().ok();
    let menu_items: Vec<MenuItem> = entries
        .iter()
        .map(|entry| MenuItem {
            entry: entry.clone(),
            preview: build_preview(entry, status.as_ref()),
        })
        .collect();

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Resolvething"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(index);
    }

    match builder.select(menu_items)? {
        FzfResult::Selected(item) => {
            cursor.update(&item.entry, &entries);
            Ok(Some(item.entry))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

fn menu_status() -> Result<MenuStatus> {
    let config = resolved_config()?;
    let directory = config.resolve_working_directory(None)?;
    let mut warnings = Vec::new();

    if !directory.exists() {
        warnings.push(format!(
            "Working directory does not exist: {}",
            format_path(&directory)
        ));
        return Ok(MenuStatus {
            config,
            directory,
            duplicate_count: None,
            conflict_count: None,
            warnings,
        });
    }

    let duplicate_count = if which::which("fclones").is_ok() {
        match super::duplicates::scan_duplicates(&directory) {
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

    let conflict_count = match super::conflicts::scan_conflicts(
        &directory,
        &config.normalized_conflict_types(&[]),
    ) {
        Ok(conflicts) => Some(conflicts.len()),
        Err(error) => {
            warnings.push(format!("Conflict scan unavailable: {}", error));
            None
        }
    };

    Ok(MenuStatus {
        config,
        directory,
        duplicate_count,
        conflict_count,
        warnings,
    })
}

fn build_preview(entry: &MenuEntry, status: Option<&MenuStatus>) -> String {
    let Some(status) = status else {
        return PreviewBuilder::new()
            .header(NerdFont::Warning, "Resolvething")
            .text("Unable to load resolvething configuration.")
            .build_string();
    };

    let working_dir = format_path(&status.directory);
    let config_path = show_config_path().unwrap_or_else(|_| "<unavailable>".to_string());
    let duplicate_count = status
        .duplicate_count
        .map(|count| count.to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let conflict_count = status
        .conflict_count
        .map(|count| count.to_string())
        .unwrap_or_else(|| "unavailable".to_string());

    let mut builder = match entry {
        MenuEntry::ResolveAll => PreviewBuilder::new()
            .header(NerdFont::Sync, "Resolve Everything")
            .field("Working Directory", &working_dir)
            .field("Duplicate Groups", &duplicate_count)
            .field("Conflicts", &conflict_count)
            .blank()
            .text("Runs duplicate cleanup first, then opens conflict resolution."),
        MenuEntry::ResolveDuplicates => PreviewBuilder::new()
            .header(NerdFont::File, "Resolve Duplicates")
            .field("Working Directory", &working_dir)
            .field("Duplicate Groups", &duplicate_count)
            .blank()
            .text("Each duplicate group uses file previews and lets you keep one copy."),
        MenuEntry::ResolveConflicts => PreviewBuilder::new()
            .header(NerdFont::GitCompare, "Resolve Conflicts")
            .field("Working Directory", &working_dir)
            .field(
                "Conflict Types",
                &status.config.conflict_file_types.join(", "),
            )
            .field("Conflicts", &conflict_count)
            .blank()
            .text("Opens your diff editor, then trashes the conflict copy when resolved."),
        MenuEntry::ConfigureWorkingDirectory => PreviewBuilder::new()
            .header(NerdFont::Folder, "Set Working Directory")
            .field("Current", &working_dir)
            .blank()
            .text("Choose the directory resolvething should scan by default.")
            .text("This uses the same file-picker flow as other ins menus."),
        MenuEntry::ConfigureConflictTypes => PreviewBuilder::new()
            .header(NerdFont::Edit, "Set Conflict Types")
            .field("Current", &status.config.conflict_file_types.join(", "))
            .blank()
            .text("Edit the list of file extensions treated as mergeable conflicts."),
        MenuEntry::EditConfig => PreviewBuilder::new()
            .header(NerdFont::Edit, "Edit Config")
            .field("Config File", &config_path)
            .blank()
            .text("Adjust the working directory, conflict file types, or editor command."),
        MenuEntry::OpenWorkingDirectory => PreviewBuilder::new()
            .header(NerdFont::FolderOpen, "Open Working Directory")
            .field("Directory", &working_dir)
            .blank()
            .text("Tries xdg-open first and falls back to printing the path."),
        MenuEntry::ShowConfigPath => PreviewBuilder::new()
            .header(NerdFont::FileConfig, "Config Path")
            .field("Path", &config_path)
            .blank()
            .text("Use this if you want to edit the file outside the menu."),
        MenuEntry::Close => PreviewBuilder::new()
            .header(NerdFont::Cross, "Close Menu")
            .text("Exit the resolvething menu."),
    };

    if !status.warnings.is_empty() {
        builder = builder.blank().separator().blank();
        for warning in &status.warnings {
            builder = builder.line(colors::YELLOW, Some(NerdFont::Warning), warning);
        }
    }

    builder.build_string()
}

fn open_working_directory() -> Result<()> {
    let config = resolved_config()?;
    let path = config.resolve_working_directory(None)?;

    if which::which("xdg-open").is_ok() {
        let _ = Command::new("xdg-open").arg(&path).spawn();
    } else {
        println!("{}", format_path(&path));
    }

    Ok(())
}
