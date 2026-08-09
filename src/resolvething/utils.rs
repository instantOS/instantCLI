use anyhow::{Context, Result, bail};
use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

/// The `.stversions` directory segment used by Syncthing for file versioning.
/// This is the single canonical definition shared by both duplicate detection
/// (`duplicates.rs`) and conflict scanning (`conflicts.rs`).
pub const STVERSIONS_DIR: &str = ".stversions";

/// Matches any Syncthing sync-conflict filename regardless of extension.
/// Compiled once and reused for every file inspected during a scan.
pub static SYNC_CONFLICT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r".*\.sync-conflict-[A-Z0-9-]*(\..*)?$").expect("invalid Syncthing conflict regex")
});

/// Strips the sync-conflict token from a filename (for reconstructing the
/// original path). Compiled once and reused for every conflict file found.
pub static SYNC_CONFLICT_REPLACE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.sync-conflict-[A-Z0-9-]*").expect("invalid Syncthing conflict replacement regex")
});

/// Matches a sync-conflict filename whose final extension is `file_type`.
/// These are built per-extension and therefore cannot be static.
pub fn sync_conflict_regex_for_type(file_type: &str) -> Regex {
    Regex::new(&format!(
        r".*\.sync-conflict-[A-Z0-9-]*\.{}$",
        regex::escape(file_type)
    ))
    .expect("invalid Syncthing conflict regex for type")
}

/// Strips the sync-conflict token *and* the extension from a filename.
pub fn sync_conflict_replace_regex_for_type(file_type: &str) -> Regex {
    Regex::new(&format!(
        r"\.sync-conflict-[A-Z0-9-]*\.{}$",
        regex::escape(file_type)
    ))
    .expect("invalid Syncthing conflict replacement regex for type")
}

/// Move `path` to the user's trash using `trash`, `gio trash`, or a manual
/// XDG fallback in that order. A path that no longer exists is a no-op.
pub fn trash_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

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
/// needed. Minimal fallback for when neither `trash` nor `gio` works.
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
    // home): copy then delete. Directories are not handled here.
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

/// Build a diff editor [`Command`] (adds `-d` for diff mode).
pub fn editor_command(configured_editor: Option<&str>) -> Result<Command> {
    let mut command = plain_editor_command(configured_editor)?;
    command.arg("-d");
    Ok(command)
}

/// Build a plain editor [`Command`] without extra flags.
pub fn plain_editor_command(configured_editor: Option<&str>) -> Result<Command> {
    let raw = configured_editor
        .filter(|v| !v.trim().is_empty())
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

/// Rename `from` to `to` without replacing an existing `to`. Returns
/// `Ok(true)` when the rename happened and `Ok(false)` when `to` already
/// exists (nothing was clobbered). Uses `renameat2(RENAME_NOREPLACE)` on
/// Linux glibc; other platforms fall back to a checked rename with a narrow
/// race window.
pub fn rename_noreplace(from: &Path, to: &Path) -> Result<bool> {
    rename_noreplace_impl(from, to)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn rename_noreplace_impl(from: &Path, to: &Path) -> Result<bool> {
    use nix::errno::Errno;
    use nix::fcntl::{AT_FDCWD, RenameFlags, renameat2};

    match renameat2(AT_FDCWD, from, AT_FDCWD, to, RenameFlags::RENAME_NOREPLACE) {
        Ok(()) => Ok(true),
        Err(Errno::EEXIST) => Ok(false),
        Err(err) => {
            Err(err).with_context(|| format!("renaming {} to {}", from.display(), to.display()))
        }
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn rename_noreplace_impl(from: &Path, to: &Path) -> Result<bool> {
    if to.exists() {
        return Ok(false);
    }
    std::fs::rename(from, to)
        .with_context(|| format!("renaming {} to {}", from.display(), to.display()))?;
    Ok(true)
}
