use anyhow::{Context, Result};
use regex::Regex;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use walkdir::WalkDir;

use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::config::format_path;
use super::utils::{
    STVERSIONS_DIR, SYNC_CONFLICT_REGEX, SYNC_CONFLICT_REPLACE_REGEX, editor_command,
    sync_conflict_regex_for_type, sync_conflict_replace_regex_for_type, trash_path,
};

const MAX_FILE_SIZE: u64 = 1_000_000;

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

/// Scan for conflict files. If `file_types` is empty, every Syncthing
/// conflict file with text-like content is considered (the validity check
/// applied later filters out binary/oversized payloads). Otherwise, only
/// conflicts whose final extension is in the list are returned.
pub fn scan_conflicts(directory: &Path, file_types: &[String]) -> Result<Vec<Conflict>> {
    let mut conflicts = Vec::new();

    if file_types.is_empty() {
        scan_walk(
            directory,
            &SYNC_CONFLICT_REGEX,
            &SYNC_CONFLICT_REPLACE_REGEX,
            "",
            &mut conflicts,
        )?;
    } else {
        for file_type in file_types {
            let regex = sync_conflict_regex_for_type(file_type);
            let replace_regex = sync_conflict_replace_regex_for_type(file_type);
            let suffix = format!(".{file_type}");
            scan_walk(directory, &regex, &replace_regex, &suffix, &mut conflicts)?;
        }
    }

    conflicts.sort_by(|a, b| a.modified.cmp(&b.modified));
    conflicts.dedup_by(|a, b| a.modified == b.modified);
    Ok(conflicts)
}

fn scan_walk(
    directory: &Path,
    regex: &Regex,
    replace_regex: &Regex,
    suffix: &str,
    conflicts: &mut Vec<Conflict>,
) -> Result<()> {
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

        // Skip files whose paths are not valid UTF-8 rather than aborting the
        // entire scan. Non-UTF-8 filenames cannot be Syncthing conflict files
        // (Syncthing itself requires UTF-8 paths), so nothing is lost.
        let Some(path_str) = entry.path().to_str() else {
            continue;
        };

        if regex.is_match(path_str) {
            let original = replace_regex.replace_all(path_str, suffix).to_string();
            conflicts.push(Conflict {
                original: PathBuf::from(original),
                modified: PathBuf::from(path_str),
            });
        }
    }
    Ok(())
}

fn file_is_valid(path: &Path) -> bool {
    if !path.exists() || !path.is_file() {
        return false;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() >= MAX_FILE_SIZE {
        return false;
    }
    // Stream through the file in chunks to detect NUL bytes without loading
    // the entire content into memory.
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return true,
            Ok(n) => {
                if buf[..n].contains(&0) {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
}

/// Returns `Ok(true)` when both files have identical content, `Ok(false)` when
/// they differ, and `Err` when either file cannot be read. Propagating errors
/// (rather than silently treating unreadable files as equal) prevents the
/// conflict resolution logic from discarding a file that it cannot verify.
fn files_equal(a: &Path, b: &Path) -> Result<bool> {
    let content_a =
        std::fs::read_to_string(a).with_context(|| format!("reading {}", a.display()))?;
    let content_b =
        std::fs::read_to_string(b).with_context(|| format!("reading {}", b.display()))?;
    Ok(content_a == content_b)
}
