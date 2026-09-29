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
use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};

use super::execution::CommandRunner;
use super::execution::paths;
use crate::arch::execution::pacman::INSTANT_MIRRORLIST;
use crate::common::pacman_mirrors::MirrorList;

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
    /// Pure mode selection, testable without touching env or filesystem.
    ///
    /// The two arguments are the results of the environment and bundle probes.
    /// Those are read by [`mode`], not here, which is the whole reason this is
    /// separable from it: every branch is reachable from a unit test.
    pub fn resolve(env_forced: bool, bundle_present: bool) -> Self {
        match (env_forced, bundle_present) {
            (true, _) => Self::Strict,
            (false, true) => Self::Opportunistic,
            (false, false) => Self::Online,
        }
    }

    pub fn is_offline(self) -> bool {
        !matches!(self, Mode::Online)
    }
}

fn env_forced() -> bool {
    matches!(std::env::var(OFFLINE_ENV).as_deref(), Ok("1"))
}

/// The bundle probe: every complete bundle ships `core.db`.
fn bundle_present() -> bool {
    Path::new(BUNDLE_ROOT)
        .join("core/os/x86_64/core.db")
        .exists()
}

/// Current install mode. Cheap: one environment read and one stat.
pub fn mode() -> Mode {
    Mode::resolve(env_forced(), bundle_present())
}

/// Reject an incomplete bundle before disk changes when the install has no
/// network fallback. With working network, opportunistic mode can use it to
/// fill missing bundle content.
pub fn validate(mode: Mode, network_available: bool) -> Result<()> {
    if mode == Mode::Online {
        return Ok(());
    }
    validate_bundle_at(
        Path::new(BUNDLE_ROOT),
        Path::new(DOTFILES_SNAPSHOT),
        mode,
        network_available,
    )
}

fn validate_bundle_at(
    bundle: &Path,
    dotfiles: &Path,
    mode: Mode,
    network_available: bool,
) -> Result<()> {
    if mode == Mode::Online {
        return Ok(());
    }

    if !bundle.join("core/os/x86_64/core.db").is_file() {
        anyhow::bail!(
            "Offline bundle is incomplete: missing repository database {}. \
             Boot a complete offline ISO or remove the incomplete bundle.",
            bundle.join("core/os/x86_64/core.db").display()
        );
    }

    if mode == Mode::Opportunistic && network_available {
        return Ok(());
    }

    for repo in ["extra", "multilib", "instant"] {
        let database = bundle.join(format!("{repo}/os/x86_64/{repo}.db"));
        if !database.is_file() {
            anyhow::bail!(
                "Offline bundle is incomplete: missing repository database {}",
                database.display()
            );
        }
    }

    if !dotfiles.join(".git").exists() {
        anyhow::bail!(
            "Offline bundle is incomplete: missing git dotfiles snapshot at {}",
            dotfiles.display()
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
    /// Write this exact content (strict: `file://` only). The wizard's
    /// region answer is intentionally not consulted here — strict is
    /// deterministic — but it still shapes the finish-time refill in
    /// [`cleanup_target`].
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

/// Offline override for [`crate::arch::execution::pacman::setup_instant_repo`]:
/// `None` online (never touch an existing user-authored list), `Some`
/// content when the bundle must be prepended.
pub fn instant_mirrorlist_override() -> Option<String> {
    match mode() {
        Mode::Online => None,
        offline => Some(instant_mirrorlist_for(offline).into_owned()),
    }
}

/// The line numbers of every active non-network `Server` entry.
///
/// Bundle entries are the complement of [`MirrorEntry::is_network`], so what
/// counts as a network server is defined once. Both the key and the value are
/// read from the shared mirrorlist parse: a line only qualifies when it is an
/// active `Server` entry, and its scheme is read from the value rather than
/// searched for in the line, so a trailing comment cannot disguise a bundle
/// entry as a network one.
fn bundle_server_lines(content: &str) -> HashSet<usize> {
    MirrorList::parse(content)
        .map(|list| {
            list.servers()
                .iter()
                .filter(|server| !server.is_network())
                .map(|server| server.line_index)
                .collect()
        })
        .unwrap_or_default()
}

/// Remove every bundle `Server` line, or `None` when the list holds none.
///
/// The result ends in a newline unless it is empty.
pub fn strip_file_servers(content: &str) -> Option<String> {
    let bundle_lines = bundle_server_lines(content);
    if bundle_lines.is_empty() {
        return None;
    }

    let kept: Vec<&str> = content
        .lines()
        .enumerate()
        .filter(|(index, _)| !bundle_lines.contains(index))
        .map(|(_, line)| line)
        .collect();
    if kept.is_empty() {
        return Some(String::new());
    }
    let mut out = kept.join("\n");
    out.push('\n');
    Some(out)
}

/// Whether the given text holds at least one active network `Server` entry.
///
/// Commented `#Server` lines do not count: pacman never uses them, so a
/// stripped list holding only comments must still be refilled or the target
/// boots with no active mirror.
///
/// A list that fails to parse has no servers, so this answers `false` and the
/// caller refills — the safe direction when unsure.
pub fn has_network_mirrors(content: &str) -> bool {
    MirrorList::parse(content).is_ok_and(|list| list.has_network_mirrors())
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

    let mut mkdir = std::process::Command::new("mkdir");
    mkdir.arg("-p").arg(&target);
    executor.run(&mut mkdir)?;

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
///
/// Dotfiles are mandatory, never best-effort: a missing snapshot is fatal
/// in strict mode (there is no network to fall back to) and only skips the
/// copy in opportunistic mode, where `setup_user_dotfiles` clones from the
/// network instead. Some source must succeed or the install fails.
pub fn copy_dotfiles_snapshot(executor: &dyn CommandRunner, mode: Mode) -> Result<()> {
    if mode == Mode::Online {
        return Ok(());
    }

    let source = Path::new(DOTFILES_SNAPSHOT);
    if !source.exists() {
        if mode == Mode::Strict {
            anyhow::bail!(
                "strict offline mode has no network fallback, but the dotfiles snapshot \
                 is missing at {DOTFILES_SNAPSHOT}: dotfiles are mandatory, so the \
                 install cannot proceed"
            );
        }
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
        // No bundle lines means this file is already in its final form.
        let Some(stripped) = strip_file_servers(&content) else {
            continue;
        };
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
        assert_eq!(Mode::resolve(false, false), Mode::Online);
        assert_eq!(Mode::resolve(false, true), Mode::Opportunistic);
        assert_eq!(Mode::resolve(true, true), Mode::Strict);
        assert_eq!(Mode::resolve(true, false), Mode::Strict);
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
        let stripped = strip_file_servers(input).expect("bundle line present");
        assert!(!stripped.contains("file://"));
        assert!(stripped.contains("https://geo.mirror.pkgbuild.com"));
        assert!(stripped.contains("# comment kept"));
        // Idempotent, and says so: a second pass finds no bundle line to strip.
        assert_eq!(strip_file_servers(&stripped), None);
    }

    #[test]
    fn a_list_with_no_bundle_line_needs_no_stripping() {
        let input = "Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n";
        assert_eq!(strip_file_servers(input), None);
    }

    #[test]
    fn a_trailing_comment_mentioning_http_is_not_a_network_mirror() {
        // The scheme belongs to the server's value. Searching the whole line
        // would find the comment and count the list as networked, skipping the
        // refill and leaving the target with no active server at all.
        let content = "Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch  # see http://example.invalid\n";
        assert!(!has_network_mirrors(content));
        let stripped = strip_file_servers(content).expect("bundle line present");
        assert!(!has_network_mirrors(&stripped));
    }

    #[test]
    fn only_real_server_entries_are_stripped() {
        // `ServerArchive` and `CacheServer` are not pacman directives, so
        // neither is a bundle entry no matter what its value looks like.
        let content = "\
ServerArchive = file:///keep/me
CacheServer = https://cache.example/$repo/os/$arch
Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch
";
        let stripped = strip_file_servers(content).expect("bundle line present");
        assert!(
            stripped.contains("ServerArchive = file:///keep/me"),
            "{stripped}"
        );
        assert!(stripped.contains("CacheServer = https://cache.example"));
        assert!(!stripped.contains("offline-repo"));
    }

    #[test]
    fn strip_of_a_strict_file_only_list_leaves_no_servers() {
        let input = "Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch\n";
        let stripped = strip_file_servers(input).expect("bundle line present");
        assert_eq!(stripped, "");
        assert!(!has_network_mirrors(&stripped));
        assert!(has_network_mirrors(
            "Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n"
        ));
    }

    #[test]
    fn commented_servers_do_not_count_as_network_mirrors() {
        assert!(!has_network_mirrors(
            "#Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n"
        ));

        // A shipped file://-first list whose only network entries are
        // commented out must be refilled after stripping, or the target
        // boots with zero active servers.
        let shipped = "\
Server = file:///run/archiso/bootmnt/offline-repo/$repo/os/$arch
#Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
";
        let stripped = strip_file_servers(shipped).expect("bundle line present");
        assert!(!has_network_mirrors(&stripped));
    }

    #[test]
    fn copy_dotfiles_snapshot_treats_a_missing_snapshot_by_mode() {
        // Test hosts have no live-ISO snapshot path; if one ever does, the
        // failure legitimately cannot be simulated here.
        if !Path::new(DOTFILES_SNAPSHOT).exists() {
            // Strict has no network to fall back to: fatal, before any command.
            let runner = MockRunner::new();
            let error = copy_dotfiles_snapshot(&runner, Mode::Strict).unwrap_err();
            assert!(error.to_string().contains(DOTFILES_SNAPSHOT));
            assert!(
                runner.command_log().is_empty(),
                "must fail before running anything"
            );

            // Opportunistic: only skips the copy; the network clone covers
            // the gap in setup_user_dotfiles.
            let runner = MockRunner::new();
            copy_dotfiles_snapshot(&runner, Mode::Opportunistic).unwrap();
            assert!(runner.command_log().is_empty());
        }
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
            log.iter().any(|c| c == "mkdir -p /mnt/run/archiso/bootmnt"),
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
    fn validation_rejects_incomplete_bundle_before_installation() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("offline-repo");
        let snapshot = temp.path().join("dotfiles");

        assert!(validate_bundle_at(&bundle, &snapshot, Mode::Online, false).is_ok());
        assert!(validate_bundle_at(&bundle, &snapshot, Mode::Strict, true).is_err());

        for repo in ["core", "extra", "multilib", "instant"] {
            let directory = bundle.join(format!("{repo}/os/x86_64"));
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("{repo}.db")), "test database").unwrap();
        }

        let instant_database = bundle.join("instant/os/x86_64/instant.db");
        std::fs::remove_file(&instant_database).unwrap();
        let missing_repo = validate_bundle_at(&bundle, &snapshot, Mode::Strict, true).unwrap_err();
        assert!(missing_repo.to_string().contains("instant.db"));
        std::fs::write(instant_database, "test database").unwrap();

        assert!(validate_bundle_at(&bundle, &snapshot, Mode::Opportunistic, true).is_ok());
        let missing_snapshot =
            validate_bundle_at(&bundle, &snapshot, Mode::Opportunistic, false).unwrap_err();
        assert!(missing_snapshot.to_string().contains("dotfiles snapshot"));

        std::fs::create_dir_all(snapshot.join(".git")).unwrap();
        assert!(validate_bundle_at(&bundle, &snapshot, Mode::Strict, false).is_ok());
        assert!(validate_bundle_at(&bundle, &snapshot, Mode::Opportunistic, false).is_ok());
    }
}
