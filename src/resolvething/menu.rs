use anyhow::Result;
use std::process::Command;

use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::commands::{
    resolve_conflicts, resolve_duplicates, resolved_config, show_config_path,
};
use super::config::{ResolvethingConfig, format_path};

#[derive(Debug, Clone)]
enum MenuEntry {
    ResolveAll,
    ResolveDuplicates,
    ResolveConflicts,
    EditConfig,
    OpenWorkingDirectory,
    ShowConfigPath,
    Close,
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
            Self::EditConfig => "!__edit_config__".to_string(),
            Self::OpenWorkingDirectory => "!__open_dir__".to_string(),
            Self::ShowConfigPath => "!__config_path__".to_string(),
            Self::Close => "!__close__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let Ok((config, directory, duplicate_count, conflict_count)) = menu_status() else {
            return PreviewBuilder::new()
                .header(NerdFont::Warning, "Resolvething")
                .text("Unable to load resolvething status.")
                .build();
        };

        let working_dir = format_path(&directory);
        let config_path = show_config_path().unwrap_or_else(|_| "<unavailable>".to_string());
        match self {
            Self::ResolveAll => PreviewBuilder::new()
                .header(NerdFont::Sync, "Resolve Everything")
                .field("Working Directory", &working_dir)
                .field("Duplicate Groups", &duplicate_count.to_string())
                .field("Conflicts", &conflict_count.to_string())
                .blank()
                .text("Runs duplicate cleanup first, then opens conflict resolution.")
                .build(),
            Self::ResolveDuplicates => PreviewBuilder::new()
                .header(NerdFont::File, "Resolve Duplicates")
                .field("Working Directory", &working_dir)
                .field("Duplicate Groups", &duplicate_count.to_string())
                .blank()
                .text("Each duplicate group uses file previews and lets you keep one copy.")
                .build(),
            Self::ResolveConflicts => PreviewBuilder::new()
                .header(NerdFont::GitCompare, "Resolve Conflicts")
                .field("Working Directory", &working_dir)
                .field("Conflict Types", &config.conflict_file_types.join(", "))
                .field("Conflicts", &conflict_count.to_string())
                .blank()
                .text("Opens your diff editor, then trashes the conflict copy when resolved.")
                .build(),
            Self::EditConfig => PreviewBuilder::new()
                .header(NerdFont::Edit, "Edit Config")
                .field("Config File", &config_path)
                .blank()
                .text("Adjust the working directory, conflict file types, or editor command.")
                .build(),
            Self::OpenWorkingDirectory => PreviewBuilder::new()
                .header(NerdFont::FolderOpen, "Open Working Directory")
                .field("Directory", &working_dir)
                .blank()
                .text("Tries xdg-open first and falls back to printing the path.")
                .build(),
            Self::ShowConfigPath => PreviewBuilder::new()
                .header(NerdFont::FileConfig, "Config Path")
                .field("Path", &config_path)
                .blank()
                .text("Use this if you want to edit the file outside the menu.")
                .build(),
            Self::Close => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close Menu")
                .text("Exit the resolvething menu.")
                .build(),
        }
    }
}

pub fn resolvething_menu(debug: bool) -> Result<()> {
    let _ = debug;
    let mut cursor = MenuCursor::new();

    loop {
        let entries = vec![
            MenuEntry::ResolveAll,
            MenuEntry::ResolveDuplicates,
            MenuEntry::ResolveConflicts,
            MenuEntry::EditConfig,
            MenuEntry::OpenWorkingDirectory,
            MenuEntry::ShowConfigPath,
            MenuEntry::Close,
        ];

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("Resolvething"))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&entries) {
            builder = builder.initial_index(index);
        }

        let selection = builder.select(entries.clone())?;
        let selected = match selection {
            FzfResult::Selected(entry) => {
                cursor.update(&entry, &entries);
                entry
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
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

fn menu_status() -> Result<(ResolvethingConfig, std::path::PathBuf, usize, usize)> {
    let config = resolved_config()?;
    let directory = config.resolve_working_directory(None)?;
    let duplicate_count = super::duplicates::scan_duplicates(&directory)
        .map(|groups| groups.len())
        .unwrap_or(0);
    let conflict_count = super::conflicts::scan_conflicts(
        &directory,
        &config.normalized_conflict_types(&[]),
    )
    .map(|conflicts| conflicts.len())
    .unwrap_or(0);

    Ok((config, directory, duplicate_count, conflict_count))
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
