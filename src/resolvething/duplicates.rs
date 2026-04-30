use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::preview::{self, PreviewId};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::commands::trash_path;
use super::config::format_path;

const IGNORED_DIR_SEGMENTS: &[&str] = &[".stversions"];

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

    pub fn is_in_ignored_dir(&self) -> bool {
        path_in_ignored_dir(&self.path)
    }
}

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub files: Vec<DuplicateEntry>,
}

#[derive(Debug, Clone)]
pub struct AutoResolution {
    pub keep: Vec<PathBuf>,
    pub trash: Vec<PathBuf>,
}

/// What to do with a duplicate group:
/// - `Auto` means we can act without prompting the user.
/// - `Manual` means the user must pick which file to keep.
/// - `Skip` means we deliberately leave the group untouched (e.g., it lives
///   entirely in an ignored folder and contains no conflict files).
#[derive(Debug, Clone)]
pub enum GroupPlan {
    Auto(AutoResolution),
    Manual,
    Skip(SkipReason),
}

#[derive(Debug, Clone, Copy)]
pub enum SkipReason {
    IgnoredFolder,
}

impl DuplicateGroup {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            files: paths.into_iter().map(DuplicateEntry::new).collect(),
        }
    }

    /// Compute what to do with this group. Conflict-file dedup runs even in
    /// ignored folders; pure-version-file groups inside `.stversions` keep
    /// only the latest copy.
    pub fn plan(&self, no_auto: bool) -> GroupPlan {
        self.plan_with(no_auto, &fs_mtime_lookup)
    }

    /// Like [`plan`], but uses an injectable mtime lookup (used by tests).
    pub fn plan_with(
        &self,
        no_auto: bool,
        mtime: &dyn Fn(&Path) -> Option<SystemTime>,
    ) -> GroupPlan {
        if self.files.len() < 2 {
            return GroupPlan::Skip(SkipReason::IgnoredFolder);
        }

        let conflicts: Vec<&DuplicateEntry> = self
            .files
            .iter()
            .filter(|f| f.file_type == DuplicateFileType::SyncthingConflict)
            .collect();
        let non_conflicts: Vec<&DuplicateEntry> = self
            .files
            .iter()
            .filter(|f| f.file_type != DuplicateFileType::SyncthingConflict)
            .collect();

        // Mixed group: keep all non-conflict files, trash all conflicts.
        // This always runs automatically, regardless of `no_auto`, and even
        // inside ignored folders.
        if !conflicts.is_empty() && !non_conflicts.is_empty() {
            return GroupPlan::Auto(AutoResolution {
                keep: non_conflicts.iter().map(|e| e.path.clone()).collect(),
                trash: conflicts.iter().map(|e| e.path.clone()).collect(),
            });
        }

        // All-conflict group: keep one (lexicographically first), trash rest.
        // Also always automatic, including inside ignored folders.
        if !conflicts.is_empty() && non_conflicts.is_empty() {
            let mut sorted: Vec<&DuplicateEntry> = conflicts.clone();
            sorted.sort_by(|a, b| a.path.cmp(&b.path));
            let keep = sorted[0].path.clone();
            let trash: Vec<PathBuf> = sorted[1..].iter().map(|e| e.path.clone()).collect();
            return GroupPlan::Auto(AutoResolution {
                keep: vec![keep],
                trash,
            });
        }

        // No conflict files past this point.
        let all_in_ignored = self.files.iter().all(DuplicateEntry::is_in_ignored_dir);
        let any_in_ignored = self.files.iter().any(DuplicateEntry::is_in_ignored_dir);

        // Pure version-file group inside `.stversions`: keep the latest copy,
        // trash the rest. These are just snapshots of the same content; one
        // archived copy is enough.
        if all_in_ignored {
            let keep = pick_latest(&self.files, mtime);
            let trash: Vec<PathBuf> = self
                .files
                .iter()
                .filter(|f| f.path != keep)
                .map(|f| f.path.clone())
                .collect();
            return GroupPlan::Auto(AutoResolution {
                keep: vec![keep],
                trash,
            });
        }

        // Mixed inside/outside ignored folder with no conflicts: skip to be
        // safe (the active live file may legitimately match a stored version).
        if any_in_ignored {
            return GroupPlan::Skip(SkipReason::IgnoredFolder);
        }

        if no_auto {
            return GroupPlan::Manual;
        }

        // Existing tmp/orig auto rules.
        let regular: Vec<&DuplicateEntry> = self
            .files
            .iter()
            .filter(|f| f.file_type == DuplicateFileType::Regular)
            .collect();
        let tmps: Vec<&DuplicateEntry> = self
            .files
            .iter()
            .filter(|f| f.file_type == DuplicateFileType::Tmp)
            .collect();

        if regular.len() == 1 && regular.len() < self.files.len() {
            let keep = regular[0].path.clone();
            let trash = self
                .files
                .iter()
                .filter(|f| f.path != keep)
                .map(|f| f.path.clone())
                .collect();
            return GroupPlan::Auto(AutoResolution {
                keep: vec![keep],
                trash,
            });
        }

        if tmps.len() == self.files.len() {
            let keep = tmps[0].path.clone();
            let trash = tmps[1..].iter().map(|e| e.path.clone()).collect();
            return GroupPlan::Auto(AutoResolution {
                keep: vec![keep],
                trash,
            });
        }

        GroupPlan::Manual
    }

    /// Trash every file in this group except the ones in `keep`.
    pub fn keep_paths(&self, keep: &[PathBuf]) -> Result<usize> {
        let mut removed = 0;
        for file in &self.files {
            if !keep.iter().any(|k| k == &file.path) {
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

/// Scan for duplicates inside `directory`. We include normally-ignored
/// folders (e.g. `.stversions`) so that conflict-file duplicates can be
/// auto-resolved even there; pure non-conflict groups inside ignored
/// folders are skipped by `DuplicateGroup::plan`.
pub fn scan_duplicates(directory: &Path) -> Result<Vec<DuplicateGroup>> {
    let output = Command::new("fclones")
        .arg("group")
        .arg("--hidden")
        .arg(directory)
        .arg("--format")
        .arg("fdupes")
        .arg("--cache")
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

pub fn path_in_ignored_dir(path: &Path) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|seg| IGNORED_DIR_SEGMENTS.contains(&seg))
}

fn fs_mtime_lookup(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Pick the entry whose mtime is most recent. Falls back to the
/// lexicographically largest path when mtimes are unavailable, which works
/// well for Syncthing's `~YYYYMMDD-HHMMSS` version filename convention.
fn pick_latest(
    files: &[DuplicateEntry],
    mtime: &dyn Fn(&Path) -> Option<SystemTime>,
) -> PathBuf {
    debug_assert!(!files.is_empty());
    let mut best: &DuplicateEntry = &files[0];
    let mut best_mtime: Option<SystemTime> = mtime(&best.path);
    for entry in &files[1..] {
        let candidate_mtime = mtime(&entry.path);
        let take = match (candidate_mtime, best_mtime) {
            (Some(c), Some(b)) => c > b,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => entry.path > best.path,
        };
        if take {
            best = entry;
            best_mtime = candidate_mtime;
        }
    }
    best.path.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(paths: &[&str]) -> DuplicateGroup {
        DuplicateGroup::new(paths.iter().map(PathBuf::from).collect())
    }

    fn auto(plan: GroupPlan) -> AutoResolution {
        match plan {
            GroupPlan::Auto(a) => a,
            other => panic!("expected Auto plan, got {:?}", other),
        }
    }

    #[test]
    fn mixed_group_keeps_all_non_conflict_files_and_trashes_conflicts() {
        let g = group(&[
            "/dir/note.md",
            "/dir/note.sync-conflict-20240101-AAA.md",
            "/dir/copy.md",
            "/dir/note.sync-conflict-20240202-BBB.md",
        ]);
        let action = auto(g.plan(false));
        assert_eq!(
            action.keep,
            vec![
                PathBuf::from("/dir/note.md"),
                PathBuf::from("/dir/copy.md"),
            ]
        );
        assert_eq!(
            action.trash,
            vec![
                PathBuf::from("/dir/note.sync-conflict-20240101-AAA.md"),
                PathBuf::from("/dir/note.sync-conflict-20240202-BBB.md"),
            ]
        );
    }

    #[test]
    fn all_conflict_group_keeps_one_trashes_rest() {
        let g = group(&[
            "/dir/note.sync-conflict-20240202-BBB.md",
            "/dir/note.sync-conflict-20240101-AAA.md",
        ]);
        let action = auto(g.plan(false));
        assert_eq!(action.keep.len(), 1);
        assert_eq!(action.trash.len(), 1);
        // Lexicographic first kept.
        assert_eq!(
            action.keep[0],
            PathBuf::from("/dir/note.sync-conflict-20240101-AAA.md")
        );
    }

    #[test]
    fn mixed_group_in_ignored_folder_still_auto_resolves_conflicts() {
        let g = group(&[
            "/dir/.stversions/note.md",
            "/dir/.stversions/note.sync-conflict-20240101-AAA.md",
        ]);
        let action = auto(g.plan(false));
        assert_eq!(action.keep, vec![PathBuf::from("/dir/.stversions/note.md")]);
        assert_eq!(
            action.trash,
            vec![PathBuf::from(
                "/dir/.stversions/note.sync-conflict-20240101-AAA.md"
            )]
        );
    }

    #[test]
    fn all_conflict_group_in_ignored_folder_keeps_one() {
        let g = group(&[
            "/dir/.stversions/a.sync-conflict-20240101-AAA.md",
            "/dir/.stversions/a.sync-conflict-20240202-BBB.md",
        ]);
        let action = auto(g.plan(false));
        assert_eq!(action.keep.len(), 1);
        assert_eq!(action.trash.len(), 1);
    }

    #[test]
    fn pure_stversions_group_keeps_latest_version_by_mtime() {
        use std::time::{Duration, UNIX_EPOCH};
        let g = group(&[
            "/dir/.stversions/a~20240101-000000.md",
            "/dir/.stversions/a~20240202-000000.md",
            "/dir/.stversions/a~20240303-000000.md",
        ]);
        let mtimes = |p: &Path| match p.to_str().unwrap() {
            "/dir/.stversions/a~20240101-000000.md" => {
                Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
            }
            "/dir/.stversions/a~20240202-000000.md" => {
                Some(UNIX_EPOCH + Duration::from_secs(1_710_000_000))
            }
            "/dir/.stversions/a~20240303-000000.md" => {
                Some(UNIX_EPOCH + Duration::from_secs(1_720_000_000))
            }
            _ => None,
        };
        let action = match g.plan_with(false, &mtimes) {
            GroupPlan::Auto(a) => a,
            other => panic!("expected Auto, got {:?}", other),
        };
        assert_eq!(
            action.keep,
            vec![PathBuf::from("/dir/.stversions/a~20240303-000000.md")]
        );
        assert_eq!(action.trash.len(), 2);
    }

    #[test]
    fn pure_stversions_group_falls_back_to_path_order_without_mtimes() {
        let g = group(&[
            "/dir/.stversions/a~20240202-000000.md",
            "/dir/.stversions/a~20240101-000000.md",
            "/dir/.stversions/a~20240303-000000.md",
        ]);
        let action = match g.plan_with(false, &|_| None) {
            GroupPlan::Auto(a) => a,
            other => panic!("expected Auto, got {:?}", other),
        };
        // Without mtimes, the latest path lexicographically wins. With ISO
        // timestamps in the filename this matches the latest version.
        assert_eq!(
            action.keep,
            vec![PathBuf::from("/dir/.stversions/a~20240303-000000.md")]
        );
        assert_eq!(action.trash.len(), 2);
    }

    #[test]
    fn mixed_inside_outside_ignored_group_is_skipped() {
        let g = group(&[
            "/dir/note.md",
            "/dir/.stversions/note~20240101-000000.md",
        ]);
        assert!(matches!(
            g.plan(false),
            GroupPlan::Skip(SkipReason::IgnoredFolder)
        ));
    }

    #[test]
    fn no_auto_forces_manual_for_regular_groups() {
        let g = group(&["/dir/a.md", "/dir/b.md"]);
        assert!(matches!(g.plan(true), GroupPlan::Manual));
    }

    #[test]
    fn no_auto_still_auto_resolves_conflict_files() {
        let g = group(&[
            "/dir/a.md",
            "/dir/a.sync-conflict-20240101-AAA.md",
        ]);
        let action = auto(g.plan(true));
        assert_eq!(action.keep, vec![PathBuf::from("/dir/a.md")]);
    }

    #[test]
    fn single_regular_in_mixed_orig_tmp_group_is_auto() {
        let g = group(&["/dir/a.md", "/dir/a.md.orig", "/dir/a.md.tmp"]);
        let action = auto(g.plan(false));
        assert_eq!(action.keep, vec![PathBuf::from("/dir/a.md")]);
        assert_eq!(action.trash.len(), 2);
    }
}
