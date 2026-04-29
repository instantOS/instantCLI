use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::preview::{self, PreviewId};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::commands::trash_path;
use super::config::format_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateFileType {
    Regular,
    SyncthingConflict,
    Orig,
    Tmp,
}

impl DuplicateFileType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::SyncthingConflict => "sync-conflict",
            Self::Orig => "orig",
            Self::Tmp => "tmp",
        }
    }

    pub fn icon(self) -> NerdFont {
        match self {
            Self::Regular => NerdFont::File,
            Self::SyncthingConflict => NerdFont::Warning,
            Self::Orig => NerdFont::Archive,
            Self::Tmp => NerdFont::Clock,
        }
    }

    pub fn color(self) -> &'static str {
        match self {
            Self::Regular => colors::GREEN,
            Self::SyncthingConflict => colors::PEACH,
            Self::Orig => colors::YELLOW,
            Self::Tmp => colors::SUBTEXT0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DuplicateEntry {
    pub path: PathBuf,
    pub file_type: DuplicateFileType,
}

impl DuplicateEntry {
    pub fn new(path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let file_type = if crate::resolvething::commands::sync_conflict_regex().is_match(file_name)
        {
            DuplicateFileType::SyncthingConflict
        } else if file_name.ends_with(".orig") {
            DuplicateFileType::Orig
        } else if file_name.ends_with(".tmp") {
            DuplicateFileType::Tmp
        } else {
            DuplicateFileType::Regular
        };

        Self { path, file_type }
    }
}

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub files: Vec<DuplicateEntry>,
}

impl DuplicateGroup {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            files: paths.into_iter().map(DuplicateEntry::new).collect(),
        }
    }

    pub fn auto_keep_choice(&self) -> Option<&DuplicateEntry> {
        let regular: Vec<_> = self
            .files
            .iter()
            .filter(|file| file.file_type == DuplicateFileType::Regular)
            .collect();
        let conflicts: Vec<_> = self
            .files
            .iter()
            .filter(|file| file.file_type == DuplicateFileType::SyncthingConflict)
            .collect();
        let tmps: Vec<_> = self
            .files
            .iter()
            .filter(|file| file.file_type == DuplicateFileType::Tmp)
            .collect();

        if self.files.is_empty() {
            None
        } else if regular.len() == 1 && regular.len() < self.files.len() {
            regular.first().copied()
        } else if conflicts.len() == 1 && tmps.len() + conflicts.len() == self.files.len() {
            conflicts.first().copied()
        } else if tmps.len() == self.files.len() {
            tmps.first().copied()
        } else if conflicts.len() == self.files.len() {
            conflicts.first().copied()
        } else {
            None
        }
    }

    pub fn keep_only(&self, keep: &Path) -> Result<usize> {
        let mut removed = 0;
        for file in &self.files {
            if file.path != keep {
                trash_path(&file.path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[derive(Debug, Clone)]
pub enum DuplicateChoice {
    Keep(DuplicateEntry),
    Skip,
}

impl crate::menu_utils::FzfSelectable for DuplicateChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Keep(file) => format!(
                "{} {} [{}]",
                format_icon_colored(file.file_type.icon(), file.file_type.color()),
                format_path(&file.path),
                file.file_type.label()
            ),
            Self::Skip => format!("{} Skip Group", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            Self::Keep(file) => file.path.to_string_lossy().to_string(),
            Self::Skip => "!__skip__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Keep(_file) => {
                FzfPreview::Command(preview::preview_command(PreviewId::FileSuggestion))
            }
            Self::Skip => PreviewBuilder::new()
                .header(NerdFont::Cross, "Skip Group")
                .text("Leave this duplicate group untouched for now.")
                .build(),
        }
    }
}

pub fn scan_duplicates(directory: &Path) -> Result<Vec<DuplicateGroup>> {
    let output = Command::new("fclones")
        .arg("group")
        .arg("--hidden")
        .arg(directory)
        .arg("--format")
        .arg("fdupes")
        .arg("--cache")
        .arg("--exclude")
        .arg("**/.stversions/**")
        .output()
        .with_context(|| format!("running fclones in {}", directory.display()))?;

    if !output.status.success() {
        bail!("fclones failed with status {}", output.status);
    }

    let stdout = String::from_utf8(output.stdout).context("parsing fclones output as UTF-8")?;
    let mut groups = Vec::new();
    let mut current = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.is_empty() {
                groups.push(DuplicateGroup::new(std::mem::take(&mut current)));
            }
            continue;
        }

        current.push(PathBuf::from(trimmed));
    }

    if !current.is_empty() {
        groups.push(DuplicateGroup::new(current));
    }

    Ok(groups)
}
