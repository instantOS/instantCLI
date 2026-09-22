//! Offline bundle detection and offline-aware mirrorlist shaping.
//!
//! The offline ISO carries a local pacman repository at [`BUNDLE_ROOT`]
//! (plain files injected into the ISO after mkarchiso, see
//! `instantOS/offlineiso.md`). Every network-adjacent installer decision
//! consults [`mode()`]:
//!
//! * [`Mode::Online`] — no bundle: current behaviour, network required.
//! * [`Mode::Opportunistic`] — bundle present: `file://` servers first with
//!   https mirrors below, so a present network heals bundle gaps.
//! * [`Mode::Strict`] — `INS_OFFLINE=1`: `file://` only, the network is
//!   never consulted (deterministic tests).
//!
//! Functions that touch the system take an explicit [`Mode`] parameter so
//! unit tests can exercise every branch without env or filesystem setup.

use std::borrow::Cow;
use std::path::Path;

use anyhow::{Context, Result};

use super::execution::CommandRunner;
use super::execution::paths;
use crate::common::pacman::INSTANT_MIRRORLIST;

/// Root of the offline package bundle on the live system.
pub const BUNDLE_ROOT: &str = "/run/archiso/bootmnt/offline-repo";

/// The boot mount that is bind-mounted into the target so the chroot sees
/// the same `file://` paths as the live system.
pub const BUNDLE_MOUNT: &str = "/run/archiso/bootmnt";

/// `INS_OFFLINE=1` forces strict networkless mode (used by tests).
pub const OFFLINE_ENV: &str = "INS_OFFLINE";

/// Bundled dotfiles snapshot shipped by every instantOS ISO build.
pub const DOTFILES_SNAPSHOT: &str = "/usr/share/instantos/build-inputs/dotfiles";

/// Stock Arch mirror used when a strict install would otherwise leave a
/// mirrorlist without a single network server.
pub const DEFAULT_ARCH_MIRROR: &str = "Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch";

/// Where the installer gets its packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Network required: current behaviour.
    Online,
    /// Bundle present: `file://` first, https heals gaps.
    Opportunistic,
    /// `INS_OFFLINE=1`: never touch the network.
    Strict,
}

impl Mode {
    pub fn is_offline(self) -> bool {
        !matches!(self, Mode::Online)
    }
}

/// Pure mode selection, testable without touching env or filesystem.
pub fn resolve(env_forced: bool, bundle_present: bool) -> Mode {
    match (env_forced, bundle_present) {
        (true, _) => Mode::Strict,
        (false, true) => Mode::Opportunistic,
        (false, false) => Mode::Online,
    }
}

fn env_forced() -> bool {
    matches!(std::env::var(OFFLINE_ENV).as_deref(), Ok("1"))
}

/// The bundle probe: every complete bundle ships `core.db`.
pub fn bundle_present() -> bool {
    Path::new(BUNDLE_ROOT)
        .join("core/os/x86_64/core.db")
        .exists()
}

/// Current install mode. Cheap: one environment read and one stat.
pub fn mode() -> Mode {
    resolve(env_forced(), bundle_present())
}

/// Strict mode without a bundle cannot install anything: fail fast with a
/// message that names the problem instead of a hundred pacman errors.
pub fn validate(mode: Mode) -> Result<()> {
    if mode == Mode::Strict && !bundle_present() {
        anyhow::bail!(
            "{OFFLINE_ENV}=1 forces a networkless install but no bundle was found at \
             {BUNDLE_ROOT}. Boot the offline ISO or unset {OFFLINE_ENV}."
        );
    }
    Ok(())
}

/// The `Server` line that reaches the bundle through pacman's `$repo` and
/// `$arch` templates. The same template serves every bundled repository
/// (`core`, `extra`, `multilib`, `instant`).
pub fn file_server_line() -> String {
    format!("Server = file://{BUNDLE_ROOT}/$repo/os/$arch")
}

/// `[instant]` mirrorlist content for a given mode.
pub fn instant_mirrorlist_for(mode: Mode) -> Cow<'static, str> {
    match mode {
        Mode::Online => Cow::Borrowed(INSTANT_MIRRORLIST),
        Mode::Opportunistic => Cow::Owned(format!(
            "# offline bundle: local packages first, network mirrors heal gaps\n{}\n{INSTANT_MIRRORLIST}",
            file_server_line()
        )),
        Mode::Strict => Cow::Owned(format!(
            "# {OFFLINE_ENV}: local bundle only\n{}\n",
            file_server_line()
        )),
    }
}

/// What [`super::execution::base`] should do to the live mirrorlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorlistAction {
    /// Fetch a mirrorlist from the network and write it (online).
    Fetch,
    /// Keep whatever is on disk: the offline ISO ships a `file://`-first
    /// mirrorlist and the region question was skipped.
    Keep,
    /// Write this exact content (strict: `file://` only).
    Replace(String),
}

/// Mirrorlist handling for a given mode.
pub fn arch_mirrorlist_action(mode: Mode) -> MirrorlistAction {
    match mode {
        Mode::Online => MirrorlistAction::Fetch,
        Mode::Opportunistic => MirrorlistAction::Keep,
        Mode::Strict => MirrorlistAction::Replace(format!("{}\n", file_server_line())),
    }
}

/// Offline override for [`crate::common::pacman::setup_instant_repo`]:
/// `None` online (never touch an existing user-authored list), `Some`
/// content when the bundle must be prepended.
pub fn instant_mirrorlist_override() -> Option<String> {
    match mode() {
        Mode::Online => None,
        offline => Some(instant_mirrorlist_for(offline).into_owned()),
    }
}

fn is_file_server_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("Server") && trimmed.contains("file://")
}

/// Remove every `Server = file://...` bundle line. Idempotent; the result
/// always ends in a newline unless it is empty.
pub fn strip_file_servers(content: &str) -> String {
    let kept: Vec<&str> = content
        .lines()
        .filter(|line| !is_file_server_line(line))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let mut out = kept.join("\n");
    out.push('\n');
    out
}

/// Whether any uncommented http(s) `Server` line remains.
pub fn has_network_mirrors(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        (trimmed.starts_with("Server") || trimmed.starts_with("#Server"))
            && trimmed.contains("http")
    })
}

/// What to refill a target file with when stripping the bundle's `file://`
/// lines would leave it without any server at all.
#[derive(Clone, Copy)]
enum Restore {
    /// The wizard-selected region's bundled mirrorlist, falling back to the
    /// stock Arch mirror when no region or snapshot is available.
    ArchRegion,
    /// The instant mirror set (region-independent).
    InstantMirrors,
    /// Nothing: stripping a network block is the intended end state.
    None,
}

/// Files on the target that may reference the bundle, with the refills to
/// restore when stripping would leave them without any server.
const TARGET_PATHS: &[(&str, Restore)] = &[
    ("/etc/pacman.d/mirrorlist", Restore::ArchRegion),
    ("/etc/pacman.d/instantmirrorlist", Restore::InstantMirrors),
    ("/etc/pacman.conf", Restore::None),
];

/// Bind the live boot mount into the target so chroot processes resolve the
/// same `file://` paths as the live system. Idempotent: `setup_chroot` runs
/// once per chroot step and must never stack a second bind.
pub fn bind_bundle(executor: &dyn CommandRunner, mode: Mode) -> Result<()> {
    if mode == Mode::Online {
        return Ok(());
    }

    let target = paths::chroot_path(BUNDLE_MOUNT);

    if executor.dry_run() {
        println!("[DRY RUN] mount --bind {BUNDLE_MOUNT} {}", target.display());
        return Ok(());
    }

    let mut check = std::process::Command::new("findmnt");
    check.arg("-rn").arg(&target);
    if let Some(output) = executor.run_with_output(&mut check)?
        && output.status.success()
    {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        let mut mkdir = std::process::Command::new("mkdir");
        mkdir.arg("-p").arg(parent);
        executor.run(&mut mkdir)?;
    }

    let mut mount = std::process::Command::new("mount");
    mount.args(["--bind", BUNDLE_MOUNT]).arg(&target);
    executor.run(&mut mount)?;
    println!(
        "Bound the offline bundle into the target at {}.",
        target.display()
    );
    Ok(())
}

/// Bring the bundled dotfiles snapshot into the target so the chroot can
/// clone it without network access. Idempotent: `setup_chroot` runs once
/// per chroot step.
pub fn copy_dotfiles_snapshot(executor: &dyn CommandRunner, mode: Mode) -> Result<()> {
    if mode == Mode::Online {
        return Ok(());
    }

    let source = Path::new(DOTFILES_SNAPSHOT);
    if !source.exists() {
        println!(
            "Warning: offline dotfiles snapshot missing at {DOTFILES_SNAPSHOT}; \
             dotfiles will be cloned from the network instead."
        );
        return Ok(());
    }

    let target = paths::chroot_path(DOTFILES_SNAPSHOT);
    if target.exists() {
        return Ok(());
    }

    if executor.dry_run() {
        println!("[DRY RUN] cp -a {DOTFILES_SNAPSHOT} {}", target.display());
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        let mut mkdir = std::process::Command::new("mkdir");
        mkdir.arg("-p").arg(parent);
        executor.run(&mut mkdir)?;
    }

    let mut cp = std::process::Command::new("cp");
    cp.arg("-a").arg(source);
    if let Some(parent) = target.parent() {
        cp.arg(parent);
    }
    executor.run(&mut cp)?;
    println!("Copied the offline dotfiles snapshot into the target.");
    Ok(())
}

/// Finish-time cleanup after a full offline installation: strip the bundle
/// server lines from the target's pacman files (restoring the selected
/// region's mirrorlist — or stock defaults — where stripping would leave no
/// server at all) and release the bind.
pub fn cleanup_target(
    executor: &dyn CommandRunner,
    mode: Mode,
    region: Option<&str>,
) -> Result<()> {
    if mode == Mode::Online {
        return Ok(());
    }

    for &(path, restore) in TARGET_PATHS {
        let target = paths::chroot_path(path);
        if !target.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&target)
            .with_context(|| format!("Failed to read {}", target.display()))?;
        let stripped = strip_file_servers(&content);
        if stripped == content {
            continue;
        }
        let final_content = match restore {
            // Stripping a network block is the intended end state.
            Restore::None => stripped,
            // Opportunistic installs inherit the region list; it survives.
            _ if has_network_mirrors(&stripped) => stripped,
            // Strict installs ran file://-only: refill so the target keeps a
            // usable mirrorlist on first boot.
            restore => {
                let fill = restore_content(restore, region);
                let fill = fill.trim_end();
                if stripped.trim().is_empty() {
                    format!("{fill}\n")
                } else {
                    format!("{}\n{fill}\n", stripped.trim_end())
                }
            }
        };
        std::fs::write(&target, final_content)
            .with_context(|| format!("Failed to write {}", target.display()))?;
        println!(
            "Removed offline bundle references from {}.",
            target.display()
        );
    }

    let bind = paths::chroot_path(BUNDLE_MOUNT);
    let mut umount = std::process::Command::new("umount");
    umount.arg(&bind);
    match executor.run(&mut umount) {
        Ok(()) => println!("Released the offline bundle bind at {}.", bind.display()),
        // The install itself succeeded; a stuck bind dies with the live
        // session on reboot and must not fail the finish line.
        Err(e) => println!("Warning: failed to unmount {}: {e}", bind.display()),
    }
    Ok(())
}

/// Content to refill a stripped file with. Only the Arch mirrorlist is
/// region-aware; the instant mirror set is fixed and pacman.conf gets nothing.
fn restore_content(kind: Restore, region: Option<&str>) -> String {
    match kind {
        Restore::ArchRegion => region
            .and_then(
                |name| match crate::arch::mirrors::bundled_region_mirrorlist(name) {
                    Ok(list) => Some(list),
                    Err(e) => {
                        println!("Warning: {e:#}; restoring the stock Arch mirror instead.");
                        None
                    }
                },
            )
            .unwrap_or_else(|| DEFAULT_ARCH_MIRROR.to_string()),
        Restore::InstantMirrors => INSTANT_MIRRORLIST.to_string(),
        Restore::None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::execution::mock::MockRunner;

    #[test]
    fn resolve_picks_the_expected_mode() {
        assert_eq!(resolve(false, false), Mode::Online);
        assert_eq!(resolve(false, true), Mode::Opportunistic);
        assert_eq!(resolve(true, true), Mode::Strict);
        assert_eq!(resolve(true, false), Mode::Strict);
    }

    #[test]
    fn restore_content_fills_each_kind() {
        // No region selected (or no bundle on the test host): stock mirror.
        assert_eq!(
            restore_content(Restore::ArchRegion, None),
            DEFAULT_ARCH_MIRROR
        );
        assert_eq!(
            restore_content(Restore::InstantMirrors, None),
            INSTANT_MIRRORLIST
        );
        assert_eq!(restore_content(Restore::None, None), "");
    }

    #[test]
    fn file_server_line_uses_the_pacman_templates() {
        assert_eq!(
            file_server_line(),
            "Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch"
        );
    }

    #[test]
    fn online_instant_mirrorlist_is_untouched() {
        assert_eq!(
            instant_mirrorlist_for(Mode::Online).as_ref(),
            INSTANT_MIRRORLIST
        );
    }

    #[test]
    fn opportunistic_instant_mirrorlist_precedes_the_bundle() {
        let content = instant_mirrorlist_for(Mode::Opportunistic);
        let file_pos = content.find("file://").expect("bundle line present");
        let https_pos = content
            .find("https://instantos.io/packages")
            .expect("network mirror present");
        assert!(file_pos < https_pos, "bundle must come first");
    }

    #[test]
    fn strict_instant_mirrorlist_is_bundle_only() {
        let content = instant_mirrorlist_for(Mode::Strict);
        assert!(content.contains("file://"));
        assert!(!content.contains("https://"), "no network fallback");
    }

    #[test]
    fn arch_mirrorlist_action_per_mode() {
        assert_eq!(
            arch_mirrorlist_action(Mode::Online),
            MirrorlistAction::Fetch
        );
        assert_eq!(
            arch_mirrorlist_action(Mode::Opportunistic),
            MirrorlistAction::Keep
        );
        match arch_mirrorlist_action(Mode::Strict) {
            MirrorlistAction::Replace(content) => {
                assert!(content.contains("file://"));
                assert!(!content.contains("https://"));
            }
            other => panic!("expected Replace, got {other:?}"),
        }
    }

    #[test]
    fn strip_removes_bundle_lines_and_keeps_network_mirrors() {
        let input = "\
## Arch mirrorlist
Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
# comment kept
";
        let stripped = strip_file_servers(input);
        assert!(!stripped.contains("file://"));
        assert!(stripped.contains("https://geo.mirror.pkgbuild.com"));
        assert!(stripped.contains("# comment kept"));
        assert_eq!(strip_file_servers(&stripped), stripped, "idempotent");
    }

    #[test]
    fn strip_of_a_strict_file_only_list_leaves_no_servers() {
        let input = "Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch\n";
        let stripped = strip_file_servers(input);
        assert_eq!(stripped, "");
        assert!(!has_network_mirrors(&stripped));
        assert!(has_network_mirrors(
            "Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n"
        ));
    }

    #[test]
    fn bind_bundle_is_skipped_online_and_mounts_when_offline() {
        let online = MockRunner::new();
        bind_bundle(&online, Mode::Online).unwrap();
        assert!(online.command_log().is_empty());

        let offline = MockRunner::new();
        bind_bundle(&offline, Mode::Strict).unwrap();
        let log = offline.command_log();
        assert!(
            log.iter().any(|c| c.starts_with("findmnt")),
            "must check for an existing bind: {log:?}"
        );
        assert!(
            log.iter().any(|c| c.starts_with("mkdir -p")),
            "must create the bind target: {log:?}"
        );
        assert!(
            log.iter()
                .any(|c| c.starts_with("mount --bind /run/archiso/bootmnt")),
            "must bind the boot mount: {log:?}"
        );
    }

    #[test]
    fn cleanup_releases_the_bind_and_is_a_noop_online() {
        let online = MockRunner::new();
        cleanup_target(&online, Mode::Online, None).unwrap();
        assert!(online.command_log().is_empty());

        let offline = MockRunner::new();
        cleanup_target(&offline, Mode::Opportunistic, None).unwrap();
        let log = offline.command_log();
        assert!(
            log.iter()
                .any(|c| c.starts_with("umount /mnt/run/archiso/bootmnt")),
            "must release the bind: {log:?}"
        );
    }

    #[test]
    fn strict_mode_without_a_bundle_fails_validation() {
        // Test hosts have no live-ISO bundle path; if one ever does, the
        // strict validation legitimately passes there.
        if !bundle_present() {
            assert!(validate(Mode::Strict).is_err());
        }
        assert!(validate(Mode::Opportunistic).is_ok());
        assert!(validate(Mode::Online).is_ok());
    }
}
