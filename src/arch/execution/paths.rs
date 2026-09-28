//! Where the installer keeps its own state, and where the target's lives.
//!
//! Two sets of paths exist and must not be confused:
//!
//! * **Target** paths (`CONFIG_FILE`, `STATE_FILE`, `LOG_FILE`) are the
//!   canonical `/etc/instant/…` and `/var/log/instantos/…` locations. They are
//!   always what the *installed* system sees, and they are what the host
//!   reaches through [`chroot_path`].
//! * **Host** paths are where this installer run reads and writes its own
//!   state: the questions file, resumable execution state, the upload record
//!   and the log.
//!
//! On the live ISO both sets coincide, because the live system's `/etc` and
//! `/var/log` are throwaway RAM: the ISO is discarded on reboot, so writing
//! the source system's files there costs nothing and everything can live in
//! the familiar place. On a running Arch or instantOS system they must not
//! coincide — `/etc/instant/questions.toml` and
//! `/var/log/instantos/install.log` are the *source* system's real files, and
//! overwriting them would destroy the machine the user is installing from.
//!
//! So a non-live host gets [`EPHEMERAL_STATE_ROOT`], a `tmpfs` directory that
//! vanishes on reboot, and keeps every host write inside it. The one
//! exception is deliberate and documented: [`dry_run_flag`] is honoured only
//! on a live ISO, because on a real system a stray leftover file would
//! silently turn every future install into a no-op.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Target-side (and live-ISO host-side) canonical paths. These are what the
/// chroot hand-off and the post-install cleanup address, and they must not
/// change: the target's copy of the configuration is read back through
/// `--questions-file /etc/instant/install_config.toml`.
pub const CONFIG_FILE: &str = "/etc/instant/install_config.toml";
pub const STATE_FILE: &str = "/etc/instant/install_state.toml";
pub const LOG_FILE: &str = "/var/log/instantos/install.log";

/// File name of the support-report upload record. Only ever written on the
/// host, so it has no target-side path: the record describes the host's log of
/// the run that produced it, and nothing inside the target needs it.
const UPLOAD_STATE_FILE: &str = "upload_state.toml";

/// Live-ISO-only opt-in file that forces dry-run mode. See the module
/// documentation for why it is not honoured on a running system.
pub const DRY_RUN_FLAG: &str = "/etc/instant/installdryrun";

pub const CHROOT_MOUNT: &str = "/mnt";

/// Host-side installer state on a running system. `tmpfs` on every
/// distribution this project targets, so nothing here survives a reboot —
/// which is exactly the lifetime an in-progress install needs.
pub const EPHEMERAL_STATE_ROOT: &str = "/run/ins-install";

/// Host-side state directory below [`CHROOT_MOUNT`]; `tmpfs` on the live ISO.
const LIVE_HOST_STATE_DIR: &str = "/etc/instant";

/// File name of the questions file inside whichever host state directory
/// [`host_state_dir`] selects.
const QUESTIONS_FILE_NAME: &str = "questions.toml";

/// A path inside the target, reachable from the host.
pub fn chroot_path(path: &str) -> PathBuf {
    PathBuf::from(CHROOT_MOUNT).join(path.trim_start_matches('/'))
}

/// Whether the installer may rewrite the host's own system configuration.
///
/// Only true on a live ISO. Inside the chroot `/etc` *is* the target's own
/// configuration, so the answer there is always yes — the chroot-side steps
/// (`Config`, `Bootloader`, `Post`) are supposed to write it.
///
/// Routed through [`crate::arch::host::HostProfile`] rather than calling
/// `is_live_iso` directly, so the `INS_HOST_ENV` test override reaches this
/// too. A test override that only half-applies would leave the live-ISO
/// behaviour untestable while looking like it applied.
pub fn host_etc_is_ephemeral() -> bool {
    if super::is_chroot() {
        return true;
    }
    crate::arch::host::HostProfile::detect()
        .map(|profile| profile.etc_is_ephemeral())
        .unwrap_or_else(|_| crate::common::distro::is_live_iso())
}

/// Directory holding this run's installer state on the host.
pub fn host_state_dir() -> PathBuf {
    if host_etc_is_ephemeral() {
        PathBuf::from(LIVE_HOST_STATE_DIR)
    } else {
        PathBuf::from(EPHEMERAL_STATE_ROOT)
    }
}

/// The questions file this run reads and writes.
///
/// This is the *host* copy. `setup_chroot` copies it into the target as
/// [`CONFIG_FILE`], which is the path the chroot re-entry passes to
/// `ins arch exec --questions-file`.
pub fn host_questions_file() -> PathBuf {
    host_state_dir().join(QUESTIONS_FILE_NAME)
}

/// Resumable execution state, keyed by the configuration digest.
pub fn host_state_file() -> PathBuf {
    host_state_dir().join("install_state.toml")
}

/// Record of the most recent support-report upload.
pub fn host_upload_state_file() -> PathBuf {
    host_state_dir().join(UPLOAD_STATE_FILE)
}

/// This run's install log.
///
/// On a running host this is a distinct file from the source system's
/// `install.log`, which a full install truncates: the source system's log
/// belongs to the source system.
pub fn host_log_file() -> PathBuf {
    if host_etc_is_ephemeral() {
        PathBuf::from(LOG_FILE)
    } else {
        host_state_dir().join("install.log")
    }
}

/// The force-dry-run opt-in file, when this host honours it.
///
/// `None` on a running system: a leftover file there is a real file on the
/// source system's root, and silently turning every future install into a
/// no-op is worse than ignoring an escape hatch nobody asked for. Use
/// `ins arch exec --dry-run` instead.
pub fn dry_run_flag() -> Option<PathBuf> {
    host_etc_is_ephemeral().then(|| PathBuf::from(DRY_RUN_FLAG))
}

/// Restrict a freshly created installer state file or directory.
///
/// On a running system the questions file carries the install password, and
/// `create_dir_all`/`File::create` would apply the caller's umask — usually
/// `0022`, i.e. world-readable. A live ISO keeps the umask default because
/// everything it writes is discarded on reboot.
fn restrict(path: &Path, mode: u32) -> anyhow::Result<()> {
    if host_etc_is_ephemeral() {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|error| {
        anyhow::anyhow!(
            "Failed to restrict installer state path {}: {error}",
            path.display()
        )
    })
}

/// Create a directory for an installer state file, private on a running host.
///
/// Takes the directory rather than deriving it so a caller that owns its own
/// layout (the pacstrap configuration, in tests) writes there and nowhere
/// else.
pub fn ensure_state_dir(dir: &Path) -> anyhow::Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|error| {
            anyhow::anyhow!(
                "Failed to create installer state directory {}: {error}",
                dir.display()
            )
        })?;
        restrict(dir, 0o700)?;
    }
    Ok(())
}

/// Create this run's host state directory. See [`ensure_state_dir`].
pub fn ensure_host_state_dir() -> anyhow::Result<()> {
    ensure_state_dir(&host_state_dir())
}

/// Write an installer state file, private on a running host.
///
/// Takes the full path for the same reason [`ensure_state_dir`] takes the
/// directory: the file is written where the caller asked, and nowhere else.
pub fn write_host_file(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_state_dir(parent)?;
    }
    fs::write(path, contents)
        .map_err(|error| anyhow::anyhow!("Failed to write {}: {error}", path.display()))?;
    restrict(path, 0o600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_paths_are_unchanged() {
        // The chroot hand-off and the post-install cleanup address these
        // absolute target paths; changing one breaks the contract.
        assert_eq!(CONFIG_FILE, "/etc/instant/install_config.toml");
        assert_eq!(STATE_FILE, "/etc/instant/install_state.toml");
        assert_eq!(LOG_FILE, "/var/log/instantos/install.log");
        assert_eq!(
            chroot_path(CONFIG_FILE).to_str(),
            Some("/mnt/etc/instant/install_config.toml")
        );
        assert_eq!(
            chroot_path(STATE_FILE).to_str(),
            Some("/mnt/etc/instant/install_state.toml")
        );
        assert_eq!(
            chroot_path(LOG_FILE).to_str(),
            Some("/mnt/var/log/instantos/install.log")
        );
    }

    #[test]
    fn the_chroot_hand_off_config_path_is_the_target_path() {
        // `setup_chroot` copies the host questions file here and the chroot
        // re-entry passes exactly this path to `ins arch exec`.
        let target = chroot_path(CONFIG_FILE);
        assert!(target.starts_with(CHROOT_MOUNT));
        assert!(CONFIG_FILE.starts_with('/'));
    }

    #[test]
    fn state_writes_land_only_where_the_caller_asked() {
        // The pacstrap configuration lives in a directory this run owns, and
        // writing it must not drag the shared state directory into existence
        // as a side effect.
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("owned");
        let file = nested.join("derived.conf");
        write_host_file(&file, "[options]\n").unwrap();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[options]\n");
        assert!(nested.is_dir());
    }

    #[test]
    fn host_paths_stay_under_one_root() {
        // Whatever the host is, no host state file may escape the single
        // state directory chosen for it.
        for file in [
            host_questions_file(),
            host_state_file(),
            host_upload_state_file(),
        ] {
            assert!(
                file.parent() == Some(host_state_dir().as_path()),
                "{file:?} escaped {}",
                host_state_dir().display()
            );
        }

        // The log is the one path a live ISO keeps in its traditional place.
        if host_etc_is_ephemeral() {
            assert_eq!(host_log_file(), PathBuf::from(LOG_FILE));
        } else {
            assert!(
                host_log_file().starts_with(host_state_dir()),
                "{} escaped {}",
                host_log_file().display(),
                host_state_dir().display()
            );
        }
    }

    #[test]
    fn the_dry_run_flag_is_only_read_from_a_throwaway_etc() {
        // On this test host `/etc/instant/installdryrun` does not exist, so
        // the live ISO is the only environment that reports a flag. The
        // invariant that matters: a non-live host never reads it.
        match dry_run_flag() {
            Some(flag) => assert_eq!(flag, PathBuf::from(DRY_RUN_FLAG)),
            None => assert!(!host_etc_is_ephemeral()),
        }
    }
}
