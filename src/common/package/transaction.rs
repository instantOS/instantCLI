//! Recognize contention from the manager's own result without modifying lock
//! files. Keep diagnostics visible while retaining them for
//! the recovery dialog.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use super::PackageManager;

#[derive(Debug, thiserror::Error)]
#[error("{manager} could not acquire its package database lock.\n{detail}")]
pub struct PackageDatabaseLocked {
    pub manager: PackageManager,
    pub detail: String,
}

pub(super) fn run_package_command(
    manager: PackageManager,
    program: &str,
    args: &[&str],
) -> Result<()> {
    // Pacman's existence-based lock is safe to check before asking for sudo.
    // Other managers use persistent files, kernel locks, or a daemon: let the
    // manager attempt the operation and interpret its actual result instead.
    if matches!(manager, PackageManager::Pacman | PackageManager::Aur)
        && std::path::Path::new("/var/lib/pacman/db.lck").try_exists()?
    {
        return Err(PackageDatabaseLocked {
            manager,
            detail: "/var/lib/pacman/db.lck exists. Another package manager may be running, or a previous operation may have left a stale lock.".to_string(),
        }.into());
    }
    let mut command = Command::new(program);
    command.args(args).env("LC_ALL", "C");
    run_command(manager, &mut command, &mut std::io::stderr())
}

fn run_command(
    manager: PackageManager,
    command: &mut Command,
    diagnostics: &mut impl Write,
) -> Result<()> {
    let mut child = command
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start {manager}"))?;
    let mut stderr = child
        .stderr
        .take()
        .context("Missing package manager stderr")?;
    let mut captured = Vec::new();
    let mut buffer = [0; 8192];
    // Drain stderr on this thread while stdout and stdin remain inherited.
    // Bound retained output, but forward every byte (including prompts).
    let read_result = (|| -> std::io::Result<()> {
        loop {
            let count = match stderr.read(&mut buffer) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if count == 0 {
                break;
            }
            // A disconnected output must not leave the child blocked on its pipe.
            let _ = diagnostics.write_all(&buffer[..count]);
            let _ = diagnostics.flush();
            captured.extend_from_slice(&buffer[..count]);
            if captured.len() > 65536 {
                captured.drain(..captured.len() - 65536);
            }
        }
        Ok(())
    })();
    drop(stderr);
    let status = child.wait().context("Failed to wait for package manager")?;
    read_result.context("Failed to read package manager diagnostics")?;
    if status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&captured).trim().to_owned();
    if is_lock_contention(manager, status.code(), &detail) {
        return Err(PackageDatabaseLocked { manager, detail }.into());
    }
    bail!("{manager} exited with {status}.\n{detail}")
}

fn is_lock_contention(manager: PackageManager, code: Option<i32>, diagnostics: &str) -> bool {
    if manager == PackageManager::Zypper && code == Some(7) {
        return true; // ZYPPER_EXIT_ZYPP_LOCKED
    }
    let diagnostics = diagnostics.to_ascii_lowercase();
    // Lock-opening failures can be permissions or filesystem errors. Check
    // the whole diagnostic, since managers may explain the cause on a separate line.
    if diagnostics.contains("permission denied")
        || diagnostics.contains("read-only file system")
        || diagnostics.contains("operation not permitted")
    {
        return false;
    }
    diagnostics.lines().any(|line| {
        match manager {
            PackageManager::Pacman | PackageManager::Aur => {
                line.contains("unable to lock database")
                    || line.contains("could not lock database: file exists")
            }
            PackageManager::Apt | PackageManager::Pkg => {
                line.contains("could not get lock")
                    || line.contains("unable to acquire the dpkg frontend lock")
                    || line.contains("is another process using it?")
                    || line.contains("dpkg database lock was locked by another process")
            }
            PackageManager::Dnf => {
                line.contains("already locked by")
                    || line.contains("failed to obtain rpm transaction lock")
                    || (line.contains("can't create transaction lock")
                        && line.contains("resource temporarily unavailable"))
            }
            PackageManager::Zypper => {
                line.contains("system management is locked by the application")
            }
            PackageManager::Apk => {
                line.contains("unable to lock database")
                    && (line.contains("temporary error")
                        || line.contains("resource temporarily unavailable")
                        || line.contains("try again later"))
            }
            PackageManager::Snap => line.contains("change in progress"),
            // These normally wait internally. Only classify an actual failure
            // to take a lock, never a progress message saying it is waiting.
            PackageManager::Flatpak | PackageManager::Cargo => {
                line.contains("lock")
                    && (line.contains("resource temporarily unavailable")
                        || line.contains("would block"))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_backend_contention() {
        for (manager, code, message) in [
            (
                PackageManager::Pacman,
                1,
                "error: failed to init transaction (unable to lock database)",
            ),
            (
                PackageManager::Aur,
                1,
                "error: could not lock database: File exists",
            ),
            (
                PackageManager::Apt,
                100,
                "E: Could not get lock /var/lib/dpkg/lock-frontend. It is held by process 123 (apt)",
            ),
            (
                PackageManager::Pkg,
                100,
                "E: Unable to acquire the dpkg frontend lock, is another process using it?",
            ),
            (
                PackageManager::Dnf,
                1,
                "Failed to obtain rpm transaction lock. Another transaction is in progress.",
            ),
            (PackageManager::Dnf, 1, "metadata already locked by 123"),
            (PackageManager::Zypper, 7, ""),
            (
                PackageManager::Apk,
                99,
                "ERROR: Unable to lock database: temporary error (try again later)",
            ),
            (
                PackageManager::Snap,
                1,
                "error: snap app has remove-snap change in progress",
            ),
            (
                PackageManager::Flatpak,
                1,
                "error: Locking repo: Resource temporarily unavailable",
            ),
            (
                PackageManager::Cargo,
                101,
                "failed to lock file: Resource temporarily unavailable",
            ),
        ] {
            assert!(
                is_lock_contention(manager, Some(code), message),
                "{manager}: {message}"
            );
        }
    }

    #[test]
    fn does_not_confuse_other_errors_or_waiting_with_contention() {
        for manager in [
            PackageManager::Pacman,
            PackageManager::Aur,
            PackageManager::Apt,
            PackageManager::Pkg,
            PackageManager::Dnf,
            PackageManager::Zypper,
            PackageManager::Apk,
            PackageManager::Snap,
            PackageManager::Flatpak,
            PackageManager::Cargo,
        ] {
            for message in [
                "Unable to lock database: Permission denied",
                "Could not get lock: Read-only file system",
                "Unable to acquire the dpkg frontend lock\nPermission denied",
                "unable to lock database\nRead-only file system",
                "package not found",
                "Blocking waiting for file lock on package cache",
                "",
            ] {
                assert!(
                    !is_lock_contention(manager, Some(1), message),
                    "{manager}: {message}"
                );
            }
        }
    }

    #[test]
    fn command_retains_and_forwards_lock_diagnostics() -> Result<()> {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "echo 'Could not get lock: held by process 123' >&2; exit 100",
        ]);
        let mut output = Vec::new();
        let error = run_command(PackageManager::Apt, &mut command, &mut output)
            .err()
            .context("fake command must fail")?;
        let lock = error
            .downcast_ref::<PackageDatabaseLocked>()
            .context("expected typed lock error")?;
        assert_eq!(lock.manager, PackageManager::Apt);
        assert_eq!(lock.detail, String::from_utf8_lossy(&output).trim());
        Ok(())
    }

    #[test]
    fn success_does_not_treat_diagnostics_as_failure() -> Result<()> {
        let mut command = Command::new("sh");
        command.args(["-c", "echo 'Could not get lock previously' >&2"]);
        run_command(PackageManager::Apt, &mut command, &mut Vec::new())
    }
}
