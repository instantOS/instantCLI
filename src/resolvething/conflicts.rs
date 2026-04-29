use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use walkdir::WalkDir;

use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::commands::{
    editor_command, sync_conflict_regex_for_type, sync_conflict_replace_regex_for_type, trash_path,
};
use super::config::format_path;

const MAX_FILE_SIZE: u64 = 1_000_000;
const STVERSIONS_DIR: &str = ".stversions";

#[derive(Debug, Clone)]
pub struct Conflict {
    pub original: PathBuf,
    pub modified: PathBuf,
}

impl Conflict {
    pub fn is_valid(&self) -> bool {
        self.original != self.modified
            && file_is_valid(&self.original)
            && file_is_valid(&self.modified)
    }

    pub fn resolve(&self, configured_editor: Option<&str>) -> Result<ConflictResolution> {
        if !self.is_valid() {
            return Ok(ConflictResolution::SkippedInvalid);
        }

        let mut command = editor_command(configured_editor)?;
        command.arg(&self.modified);
        command.arg(&self.original);
        command.stdin(Stdio::inherit());
        command.stdout(Stdio::inherit());
        command.stderr(Stdio::inherit());
        command
            .spawn()
            .context("launching conflict diff editor")?
            .wait()
            .context("waiting for diff editor to exit")?;

        if files_equal(&self.original, &self.modified)? {
            trash_path(&self.modified)?;
            Ok(ConflictResolution::Resolved)
        } else {
            Ok(ConflictResolution::Unresolved)
        }
    }

    pub fn preview(&self) -> String {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::GitCompare, "Syncthing Conflict")
            .field("Original", &format_path(&self.original))
            .field("Conflict", &format_path(&self.modified))
            .blank();

        if self.is_valid() {
            builder = builder.line(
                colors::GREEN,
                Some(NerdFont::Check),
                "Both files look like editable text files",
            );
        } else {
            builder = builder.line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "One side is missing, binary, or too large for safe diffing",
            );
        }

        builder.build_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    Resolved,
    Unresolved,
    SkippedInvalid,
}

#[derive(Debug, Clone)]
pub enum ConflictChoice {
    Resolve(Conflict),
    ResolveAll,
    Close,
}

impl FzfSelectable for ConflictChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Resolve(conflict) => format!(
                "{} {}",
                format_icon_colored(NerdFont::GitCompare, colors::PEACH),
                format_path(&conflict.modified)
            ),
            Self::ResolveAll => format!(
                "{} Resolve All",
                format_icon_colored(NerdFont::Sync, colors::GREEN)
            ),
            Self::Close => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Resolve(conflict) => conflict.modified.to_string_lossy().to_string(),
            Self::ResolveAll => "!__resolve_all__".to_string(),
            Self::Close => "!__close__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Resolve(conflict) => FzfPreview::Text(conflict.preview()),
            Self::ResolveAll => PreviewBuilder::new()
                .header(NerdFont::Sync, "Resolve All")
                .text("Open each remaining conflict in sequence.")
                .blank()
                .text("After you save and quit the diff editor,")
                .text("resolved conflict files are moved to trash automatically.")
                .build(),
            Self::Close => PreviewBuilder::new()
                .header(NerdFont::Cross, "Back")
                .text("Return without resolving more conflicts.")
                .build(),
        }
    }
}

pub fn scan_conflicts(directory: &Path, file_types: &[String]) -> Result<Vec<Conflict>> {
    let mut conflicts = Vec::new();

    for file_type in file_types {
        let regex = sync_conflict_regex_for_type(file_type);
        let replace_regex = sync_conflict_replace_regex_for_type(file_type);

        for entry in WalkDir::new(directory)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry
                .path()
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .any(|segment| segment == STVERSIONS_DIR)
            {
                continue;
            }

            if !entry.file_type().is_file() {
                continue;
            }

            let path_str = entry
                .path()
                .to_str()
                .context("encountered non-UTF-8 file path while scanning conflicts")?;

            if regex.is_match(path_str) {
                let original = replace_regex
                    .replace_all(path_str, &format!(".{file_type}"))
                    .to_string();
                conflicts.push(Conflict {
                    original: PathBuf::from(original),
                    modified: PathBuf::from(path_str),
                });
            }
        }
    }

    conflicts.sort_by(|a, b| a.modified.cmp(&b.modified));
    conflicts.dedup_by(|a, b| a.modified == b.modified);
    Ok(conflicts)
}

fn file_is_valid(path: &Path) -> bool {
    path.exists()
        && path.is_file()
        && std::fs::metadata(path)
            .ok()
            .map(|meta| meta.len() < MAX_FILE_SIZE)
            .unwrap_or(false)
        && std::fs::read(path)
            .ok()
            .map(|content| !content.contains(&0))
            .unwrap_or(false)
}

fn files_equal(a: &Path, b: &Path) -> Result<bool> {
    Ok(std::fs::read_to_string(a).ok() == std::fs::read_to_string(b).ok())
}
