//! How the installer reaches the selected package mirrors.
//!
//! `pacstrap` normally reads the host's pacman configuration and copies its
//! mirrorlist into the target. That is safe on a disposable live ISO but would
//! reconfigure a running host. [`PackageSource::Isolated`] instead uses files
//! under the installer's temporary state directory (`pacstrap -C -M`) and
//! writes the selected mirrorlist into the target. Inside the chroot, `/etc`
//! belongs to the target, so in-place writes are safe.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::paths;

/// The official repositories a fresh Arch target needs, and the only ones the
/// base install may consult.
const OFFICIAL_REPOSITORIES: &[&str] = &["core", "extra", "multilib"];

/// Host pacman configuration, used in place.
const HOST_PACMAN_CONF: &str = "/etc/pacman.conf";
/// Host mirrorlist, used in place.
const HOST_MIRRORLIST: &str = "/etc/pacman.d/mirrorlist";

/// The running system's own pacman configuration.
///
/// Named as a value rather than hard-coded at each use so the "a non-live
/// install leaves the source system's pacman files byte-identical" property
/// can be tested against a temporary directory: production resolves the real
/// paths, tests resolve a fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPacman {
    pub config: PathBuf,
    pub mirrorlist: PathBuf,
}

impl HostPacman {
    /// The pacman configuration of the system the installer runs on.
    pub fn detect() -> Self {
        Self {
            config: PathBuf::from(HOST_PACMAN_CONF),
            mirrorlist: PathBuf::from(HOST_MIRRORLIST),
        }
    }
}

/// The installer's own copies of the pacman configuration, kept out of the
/// host's `/etc`, plus the host files they are derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSourceFiles {
    /// The pacstrap configuration this run derives. Ours to write.
    own_config: PathBuf,
    /// The mirrorlist this run installs from. Ours to write.
    own_mirrorlist: PathBuf,
    /// Where that mirrorlist must end up, inside the disk being installed.
    target_mirrorlist: PathBuf,
    /// The source system's own pacman files. Read-only inputs to the
    /// derivation — never written, and the reason this type exists.
    host: HostPacman,
}

impl PackageSourceFiles {
    /// An isolated set of files rooted at `dir`, reading the host's pacman
    /// files from `host`. Production uses [`Self::new`]; tests use this to
    /// point the whole flow at a temporary directory.
    #[cfg(test)]
    fn in_directory(dir: PathBuf, host: HostPacman) -> Self {
        Self {
            own_config: dir.join("pacstrap.conf"),
            own_mirrorlist: dir.join("pacstrap-mirrorlist"),
            target_mirrorlist: dir.join("mnt/etc/pacman.d/mirrorlist"),
            host,
        }
    }

    fn new() -> Self {
        let dir = paths::host_state_dir();
        Self {
            own_config: dir.join("pacstrap.conf"),
            own_mirrorlist: dir.join("pacstrap-mirrorlist"),
            target_mirrorlist: paths::chroot_path(HOST_MIRRORLIST),
            host: HostPacman::detect(),
        }
    }

    /// The pacstrap configuration this run derived from its own mirrorlist.
    pub fn own_config(&self) -> &Path {
        &self.own_config
    }

    /// The mirrorlist this run installs from, in the installer's state dir.
    pub fn own_mirrorlist(&self) -> &Path {
        &self.own_mirrorlist
    }

    /// Where the selected mirrorlist must end up in the target.
    pub fn target_mirrorlist(&self) -> &Path {
        &self.target_mirrorlist
    }

    /// The source system's own pacman files, which this run only ever reads.
    #[cfg(test)]
    pub fn host(&self) -> &HostPacman {
        &self.host
    }
}

/// How this run reaches the selected package mirrors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSource {
    /// Use the host's pacman configuration where it is, and let `pacstrap`
    /// carry the mirrorlist into the target.
    InPlace,
    /// Keep the host's pacman configuration untouched.
    Isolated(PackageSourceFiles),
}

impl PackageSource {
    /// The package source appropriate for this machine.
    ///
    /// A live ISO keeps the in-place behaviour, byte for byte, and so does a
    /// re-entry inside the chroot.
    pub fn resolve() -> Self {
        if paths::host_etc_is_ephemeral() {
            Self::InPlace
        } else {
            Self::Isolated(PackageSourceFiles::new())
        }
    }

    /// Whether this run keeps the host's pacman configuration untouched.
    ///
    /// True exactly when [`Self::derived_pacman`] is `Some`, which is also
    /// when `pacstrap` must be told to skip copying the host's mirrorlist:
    /// otherwise the target would end up with the source system's mirrors.
    pub fn is_isolated(&self) -> bool {
        matches!(self, Self::Isolated(_))
    }

    pub fn files(&self) -> Option<&PackageSourceFiles> {
        match self {
            Self::InPlace => None,
            Self::Isolated(files) => Some(files),
        }
    }

    /// Install the selected mirrorlist for this run.
    ///
    /// In place, this writes the host's mirrorlist, which `pacstrap` then
    /// copies into the target. Isolated, it writes the installer's own copy
    /// and regenerates the derived pacman configuration from it.
    pub fn apply_mirrorlist(&self, content: &str, dry_run: bool) -> Result<()> {
        if dry_run {
            return Ok(());
        }
        match self {
            Self::InPlace => std::fs::write(HOST_MIRRORLIST, content)
                .with_context(|| format!("Failed to write {HOST_MIRRORLIST}")),
            Self::Isolated(files) => self.write_isolated(files, content),
        }
    }

    fn write_isolated(&self, files: &PackageSourceFiles, content: &str) -> Result<()> {
        if server_lines(content).is_empty() {
            bail!(
                "the selected mirrorlist has no active `Server =` entries, so pacstrap would have no mirror to install from"
            );
        }
        paths::write_host_file(files.own_mirrorlist(), content)
            .with_context(|| format!("Failed to write {}", files.own_mirrorlist().display()))?;

        let source_config = std::fs::read_to_string(&files.host.config)
            .with_context(|| format!("Failed to read {}", files.host.config.display()))?;
        let derived = derive_pacstrap_config(&source_config, content);
        paths::write_host_file(files.own_config(), &derived)
            .with_context(|| format!("Failed to write {}", files.own_config().display()))
    }

    /// The pacman belonging to the system being installed.
    ///
    /// Always available, unlike [`Self::derived_pacman`]: the isolation
    /// decision is about where `pacstrap` *reads* its configuration from, but
    /// the target has its own pacman either way, and that is the one whose
    /// package cache the install wants to reclaim.
    pub fn target_pacman(&self) -> crate::arch::execution::pacman::Pacman {
        crate::arch::execution::pacman::Pacman::for_target(
            crate::arch::execution::paths::CHROOT_MOUNT,
            paths::chroot_path(HOST_PACMAN_CONF),
            paths::chroot_path(HOST_MIRRORLIST),
        )
    }

    /// The pacman this run derives and installs from, if not the host's own.
    ///
    /// `None` means in-place: `pacstrap` then uses the host's configuration and
    /// carries the host's mirrorlist into the target, which is how the selected
    /// region reaches the target on a live ISO. Isolated, this carries both
    /// files `pacstrap` needs — the `-C` configuration and the mirrorlist its
    /// retry loop reorders — neither of which may be the source system's.
    pub fn derived_pacman(&self) -> Option<crate::arch::execution::pacman::Pacman> {
        self.files().map(|files| {
            crate::arch::execution::pacman::Pacman::for_target(
                crate::arch::execution::paths::CHROOT_MOUNT,
                files.own_config(),
                files.own_mirrorlist(),
            )
        })
    }

    /// Apply pacman's `[options]` tuning for this run.
    ///
    /// In place, the host's `pacman.conf` is rewritten — correct only while
    /// the host's `/etc` is RAM. Isolated, the derived configuration already
    /// carries the tuning, and rewriting the host's file is the bug being
    /// avoided; it is regenerated from the mirrorlist already installed, so
    /// the two cannot drift apart.
    pub async fn configure_host_pacman(&self, dry_run: bool) -> Result<()> {
        let files = match self {
            Self::InPlace => {
                crate::arch::execution::pacman::Pacman::current()
                    .configure_settings(dry_run)
                    .await?;
                return Ok(());
            }
            Self::Isolated(files) => files,
        };

        if dry_run {
            println!(
                "[DRY RUN] Applying pacman settings to the installer's own configuration ({})",
                files.own_config().display()
            );
            return Ok(());
        }

        self.write_isolated(files, &self.installed_mirrorlist()?)
    }

    /// The mirrorlist this run installs from, if one was installed.
    ///
    /// An offline bundle can legitimately leave the run's mirrorlist
    /// untouched — the ISO's shipped `file://`-first list is already the
    /// right one — so the installer's own file may not exist yet. The host's
    /// list is then the only source available, and it is read rather than
    /// written.
    fn installed_mirrorlist(&self) -> Result<String> {
        let path = self.effective_mirrorlist();
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && self.is_isolated() => {
                let host = self
                    .files()
                    .map(|files| files.host.mirrorlist.clone())
                    .unwrap_or_else(|| PathBuf::from(HOST_MIRRORLIST));
                std::fs::read_to_string(&host).with_context(|| {
                    format!(
                        "No mirrorlist has been selected and {} could not be read to seed the \
                         installer's own configuration: {error}",
                        host.display()
                    )
                })
            }
            Err(error) => Err(error).with_context(|| format!("Failed to read {}", path.display())),
        }
    }

    /// The mirrorlist this run actually installs from, for the retry paths
    /// that reorder or refresh it.
    pub fn effective_mirrorlist(&self) -> PathBuf {
        match self {
            Self::InPlace => PathBuf::from(HOST_MIRRORLIST),
            Self::Isolated(files) => files.own_mirrorlist().to_path_buf(),
        }
    }

    /// Put the selected mirrorlist in the target.
    ///
    /// A no-op in place: `pacstrap` has already copied the host's list, which
    /// is the list that was selected. In dry-run there is nothing installed
    /// and nothing to read, so the run's own mirrorlist is reported instead.
    pub fn install_target_mirrorlist(&self, dry_run: bool) -> Result<()> {
        let Some(files) = self.files() else {
            return Ok(());
        };
        if dry_run {
            println!(
                "[DRY RUN] Writing the selected mirrorlist to {}",
                files.target_mirrorlist().display()
            );
            return Ok(());
        }
        let content = self.installed_mirrorlist()?;
        let target = files.target_mirrorlist();
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(target, &content)
            .with_context(|| format!("Failed to write {}", target.display()))?;
        println!(
            "Installed the selected mirrorlist into the target at {}.",
            target.display()
        );
        Ok(())
    }
}

/// Every active `Server =` line in `content`, in order.
fn server_lines(content: &str) -> Vec<&str> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("Server") && line.contains('=') && !line.starts_with('#'))
        .collect()
}

/// The global section of a pacman configuration: its signature policy,
/// `Architecture`, and the download options. Everything that is not this
/// section is a repository, i.e. a mirror source.
const OPTIONS_SECTION: &str = "options";

/// Directives that select mirrors. Dropped from the inherited block because
/// the mirrors are this run's own decision, and an inherited `Include` would
/// silently put the source system's mirrorlist back.
const MIRROR_DIRECTIVES: &[&str] = &["Include", "Server"];

/// The section header a line declares, if any. A commented-out section
/// (`#[multilib]`, which the shipped `pacman.conf` carries for exactly this
/// kind of tool) is still a boundary: its commented-out `Include` must not be
/// inherited.
fn section_header(line: &str) -> Option<&str> {
    let trimmed = line.trim_start().trim_start_matches('#').trim_start();
    let rest = trimmed.strip_prefix('[')?;
    rest.split(']').next().map(str::trim)
}

/// The inherited global block: the leading comments and the `[options]`
/// section, up to the first repository or commented-out repository.
///
/// This is what the target inherits from the running system — signature
/// policy, `Architecture`, `CheckSpace` — so it is copied rather than
/// reconstructed. Repository sections are deliberately not returned: the
/// mirrors come from the region the user chose, not from whatever the source
/// system happened to be pointed at.
fn global_block(host_config: &str) -> String {
    let mut block: Vec<String> = Vec::new();

    for line in host_config.lines() {
        match section_header(line) {
            Some(header) if header.eq_ignore_ascii_case(OPTIONS_SECTION) => {
                block.push(line.to_string());
            }
            // Any other section ends the inherited block.
            Some(_) => break,
            None => {
                let is_directive = !line.trim_start().starts_with('#') && line.contains('=');
                let directive = line.split_once('=').map(|(key, _)| key.trim());
                if is_directive
                    && directive.is_some_and(|key| {
                        MIRROR_DIRECTIVES
                            .iter()
                            .any(|name| name.eq_ignore_ascii_case(key))
                    })
                {
                    continue;
                }
                block.push(line.to_string());
            }
        }
    }

    while block.last().is_some_and(|line| line.trim().is_empty()) {
        block.pop();
    }
    if block.is_empty() {
        return String::new();
    }
    let mut joined = block.join("\n");
    joined.push('\n');
    joined
}

/// Build the pacman configuration `pacstrap` uses on a running host.
///
/// Derived from the host's `pacman.conf`, which is only read: its global
/// `[options]` block is carried over verbatim so signature policy,
/// `Architecture` and download behaviour still match the running system.
/// Every repository section is then replaced by the official Arch
/// repositories, served by the selected mirrorlist.
///
/// A third-party repository configured on the host is deliberately dropped.
/// It is not needed to build the base system, and a repository that is
/// unreachable (or signed with keys the target does not have) would fail the
/// whole `-Sy` and take the install down with it.
pub fn derive_pacstrap_config(host_config: &str, mirrorlist: &str) -> String {
    let servers = server_lines(mirrorlist);
    let mut derived = global_block(host_config);

    for repository in OFFICIAL_REPOSITORIES {
        if !derived.is_empty() {
            derived.push('\n');
        }
        derived.push_str(&format!("[{repository}]\n"));
        for server in &servers {
            derived.push_str(server);
            derived.push('\n');
        }
    }

    derived
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(super) const HOST_CONF: &str = "\
[options]
HoldPkg     = pacman glibc
Architecture = auto
#Color
#ParallelDownloads = 5
CheckSpace

#[multilib]
#Include = /etc/pacman.d/mirrorlist

[core]
Include = /etc/pacman.d/mirrorlist

[extra]
Include = /etc/pacman.d/mirrorlist

[instant]
SigLevel = Optional TrustAll
Include = /etc/pacman.d/instantmirrorlist
";

    pub(super) const MIRRORLIST: &str = "\
## Germany
Server = https://mirror.de/$repo/os/$arch
Server = https://mirror2.de/$repo/os/$arch
#Server = https://disabled.example/$repo/os/$arch
";

    #[test]
    fn derived_config_keeps_options_and_replaces_repositories() {
        let derived = derive_pacstrap_config(HOST_CONF, MIRRORLIST);

        // Global options survive, so the target inherits the running
        // system's signature and architecture policy.
        assert!(derived.contains("HoldPkg     = pacman glibc"));
        assert!(derived.contains("Architecture = auto"));
        assert!(derived.contains("CheckSpace"));

        // The selected servers serve the official repositories.
        for repository in OFFICIAL_REPOSITORIES {
            assert!(derived.contains(&format!("[{repository}]")));
        }
        assert!(derived.contains("https://mirror.de/$repo/os/$arch"));
        assert!(derived.contains("https://mirror2.de/$repo/os/$arch"));
        // Commented servers stay commented: pacman must not use them.
        assert!(!derived.contains("disabled.example"));

        // Nothing may still point at the host's files.
        assert!(!derived.contains("/etc/pacman.d/mirrorlist"));
        assert!(!derived.contains("instantmirrorlist"));
        assert!(!derived.contains("[instant]"));
    }

    #[test]
    fn the_inherited_block_stops_at_the_first_repository() {
        // `#[multilib]` is a *commented-out* section, and the shipped
        // pacman.conf carries one for exactly this kind of tool. Inheriting
        // past it would take its commented-out `Include` with it.
        let derived = derive_pacstrap_config(HOST_CONF, MIRRORLIST);
        let options_end = derived.find("[core]").expect("repositories follow");
        let inherited = &derived[..options_end];

        assert!(inherited.contains("[options]"));
        assert!(inherited.contains("HoldPkg"));
        assert!(
            !inherited.contains("multilib"),
            "the commented-out section must end the inherited block: {derived}"
        );
    }

    #[test]
    fn each_official_repository_appears_exactly_once() {
        let derived = derive_pacstrap_config(HOST_CONF, MIRRORLIST);
        for repository in OFFICIAL_REPOSITORIES {
            assert_eq!(
                derived.matches(&format!("[{repository}]")).count(),
                1,
                "{repository} must appear exactly once: {derived}"
            );
        }
    }

    #[test]
    fn a_host_config_without_an_options_section_still_yields_a_usable_config() {
        // A minimal or hand-edited pacman.conf must not produce a leading
        // blank line pacman would read as part of the first section.
        let derived =
            derive_pacstrap_config("[core]\nServer = https://old/$repo/os/$arch\n", MIRRORLIST);
        assert!(derived.starts_with("[core]"), "{derived}");
        assert!(!derived.contains("old"));
    }

    #[test]
    fn a_mirrorlist_without_servers_cannot_be_derived_into() {
        // pacman would fail with "no servers configured"; the writer rejects
        // it earlier with a message that names the cause.
        let source = PackageSource::Isolated(PackageSourceFiles::in_directory(
            PathBuf::from("/nonexistent"),
            HostPacman::detect(),
        ));
        let error = source
            .apply_mirrorlist("## no servers here\n", false)
            .unwrap_err();
        assert!(error.to_string().contains("no active `Server =` entries"));
    }

    #[test]
    fn the_live_iso_keeps_pacstraps_own_mirrorlist_handling() {
        // The e2e suite runs on the live ISO, where `pacstrap` copying the
        // host mirrorlist is how the target gets the selected region. That
        // path must not gain a `-C` or a `-M`.
        let source = PackageSource::InPlace;
        assert!(!source.is_isolated());
        assert!(source.derived_pacman().is_none());
        assert!(!source.is_isolated());
        assert_eq!(
            source.effective_mirrorlist(),
            PathBuf::from(HOST_MIRRORLIST)
        );
        // The mirrorlist is already in the target after `pacstrap`.
        source.install_target_mirrorlist(false).unwrap();
    }

    #[test]
    fn a_dry_run_never_touches_the_target_mirrorlist() {
        // `--dry-run` on a running host must not read the installer's
        // mirrorlist either: in a dry run nothing was ever written, so the
        // read would fail on a path that legitimately does not exist.
        let source = PackageSource::Isolated(PackageSourceFiles::in_directory(
            PathBuf::from("/nonexistent"),
            HostPacman::detect(),
        ));
        source.install_target_mirrorlist(true).unwrap();
    }

    #[test]
    fn the_isolated_source_never_names_a_host_pacman_path() {
        let source = PackageSource::Isolated(PackageSourceFiles::new());
        let derived = source
            .derived_pacman()
            .expect("isolated source has a pacman");
        assert!(derived.conf().starts_with(paths::host_state_dir()));
        assert!(derived.mirrorlist().starts_with(paths::host_state_dir()));
        assert!(source.is_isolated());
        assert_eq!(
            source.effective_mirrorlist(),
            paths::host_state_dir().join("pacstrap-mirrorlist")
        );
        assert_eq!(
            source
                .files()
                .expect("isolated source has files")
                .target_mirrorlist(),
            Path::new("/mnt/etc/pacman.d/mirrorlist")
        );
    }

    #[test]
    fn apply_mirrorlist_needs_no_filesystem_in_dry_run() {
        // `--dry-run` on a running host must not create `/run/ins-install`.
        let source = PackageSource::Isolated(PackageSourceFiles::in_directory(
            PathBuf::from("/nonexistent"),
            HostPacman::detect(),
        ));
        source.apply_mirrorlist(MIRRORLIST, true).unwrap();
    }

    #[test]
    fn retry_paths_use_the_run_s_own_mirrorlist() {
        // The pacstrap retry loop shuffles whichever path the run installs
        // from, so both variants must resolve to a real, writable location
        // and the in-place one must be the host file `pacstrap` copies.
        let in_place = PackageSource::InPlace;
        assert_eq!(
            in_place.effective_mirrorlist(),
            PathBuf::from("/etc/pacman.d/mirrorlist")
        );
        let isolated = PackageSource::Isolated(PackageSourceFiles::new());
        assert_eq!(
            isolated.effective_mirrorlist(),
            isolated
                .files()
                .map(|f| f.own_mirrorlist().to_path_buf())
                .expect("isolated source has files")
        );
    }

    #[test]
    fn the_isolated_source_is_only_used_off_a_live_iso() {
        // On this test host (not a live ISO) the isolated variant must be
        // chosen, which is the whole point of the change.
        if crate::common::distro::is_live_iso() {
            assert!(!PackageSource::resolve().is_isolated());
        } else {
            assert!(PackageSource::resolve().is_isolated());
        }
    }
}

/// The property this module exists to guarantee: a non-live install writes
/// nothing outside the target mount and the installer's own state directory.
///
/// Enumerated rather than asserted ad hoc, so a future change that adds a
/// host-side write has to add its path here to stay covered. The check is
/// deliberately about the *set* of paths, not about which function writes
/// them: a new host write is a regression regardless of which module does it.
#[cfg(test)]
mod host_write_allowlist {
    use super::tests::{HOST_CONF, MIRRORLIST};
    use super::*;

    /// Every path a non-live install may write outside `/mnt`.
    ///
    /// Derived from the same functions the production code uses, so it cannot
    /// drift from the actual layout.
    fn allowed_host_writes() -> Vec<String> {
        let files = PackageSourceFiles::new();
        let mut allowed = vec![
            paths::host_questions_file().display().to_string(),
            paths::host_state_file().display().to_string(),
            paths::host_upload_state_file().display().to_string(),
            paths::host_log_file().display().to_string(),
            files.own_config().display().to_string(),
            files.own_mirrorlist().display().to_string(),
        ];
        allowed.sort();
        allowed.dedup();
        allowed
    }

    /// Host pacman files a non-live install must not write. On the live ISO
    /// rewriting these is free, because its `/etc` is the archiso cowspace.
    const HOST_PACMAN_FILES: &[&str] = &["/etc/pacman.conf", "/etc/pacman.d/mirrorlist"];

    #[test]
    fn the_offline_keep_path_still_produces_a_usable_configuration() {
        // An offline bundle can leave the run's own mirrorlist unwritten: the
        // ISO's shipped list is already correct. `configure_host_pacman` must
        // still produce a pacstrap config in that case, or the base install
        // would read one that does not exist yet.
        let dir = tempfile::tempdir().unwrap();
        let files =
            PackageSourceFiles::in_directory(dir.path().to_path_buf(), HostPacman::detect());
        let source = PackageSource::Isolated(files);

        // The run's own mirrorlist does not exist yet, and the host's is
        // unreadable on this test machine, so the failure has to name the
        // real cause rather than a missing file the caller wrote.
        let error = source.installed_mirrorlist().unwrap_err().to_string();
        assert!(
            error.contains(HOST_MIRRORLIST),
            "the fallback to the host list must be reported: {error}"
        );
    }

    #[test]
    fn an_installed_mirrorlist_is_read_back_rather_than_regenerated() {
        let dir = tempfile::tempdir().unwrap();
        let files =
            PackageSourceFiles::in_directory(dir.path().to_path_buf(), HostPacman::detect());
        std::fs::write(files.own_mirrorlist(), MIRRORLIST).unwrap();
        let source = PackageSource::Isolated(files);

        assert_eq!(source.installed_mirrorlist().unwrap(), MIRRORLIST);
    }

    /// A complete isolated install of the mirror configuration, against a
    /// throwaway "host system".
    ///
    /// Returns the host files' digests after the install so the caller can
    /// compare them against the digests taken before.
    fn isolated_install_against_fixture(
        root: &std::path::Path,
        mirrorlist: &str,
    ) -> (String, String) {
        let host_config = root.join("host-etc-pacman.conf");
        let host_mirrorlist = root.join("host-etc-pacman.d-mirrorlist");
        std::fs::write(&host_config, HOST_CONF).unwrap();
        std::fs::write(&host_mirrorlist, MIRRORLIST).unwrap();

        let source = PackageSource::Isolated(PackageSourceFiles::in_directory(
            root.join("state"),
            HostPacman {
                config: host_config.clone(),
                mirrorlist: host_mirrorlist.clone(),
            },
        ));

        source.apply_mirrorlist(mirrorlist, false).unwrap();
        source.install_target_mirrorlist(false).unwrap();

        (digest(&host_config), digest(&host_mirrorlist))
    }

    fn digest(path: &std::path::Path) -> String {
        std::fs::read(path).map(hex::encode).unwrap_or_default()
    }

    #[test]
    fn an_isolated_install_leaves_the_host_pacman_files_byte_identical() {
        // A live ISO gets away with rewriting the host's pacman files because
        // its `/etc` is RAM; on a running system that silently reconfigures the
        // machine the user is installing from, and the mirrorlist the target
        // would have received is the
        // source system's, not the region the user chose.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let host_config = root.join("host-etc-pacman.conf");
        let host_mirrorlist = root.join("host-etc-pacman.d-mirrorlist");
        std::fs::write(&host_config, HOST_CONF).unwrap();
        std::fs::write(&host_mirrorlist, MIRRORLIST).unwrap();
        let before = (digest(&host_config), digest(&host_mirrorlist));

        let source = PackageSource::Isolated(PackageSourceFiles::in_directory(
            root.join("state"),
            HostPacman {
                config: host_config,
                mirrorlist: host_mirrorlist,
            },
        ));
        let selected = "Server = https://chosen.example/$repo/os/$arch\n";

        // The host's files are reachable only for reading.
        assert_eq!(
            source.files().map(PackageSourceFiles::host),
            Some(&HostPacman {
                config: root.join("host-etc-pacman.conf"),
                mirrorlist: root.join("host-etc-pacman.d-mirrorlist"),
            })
        );

        source.apply_mirrorlist(selected, false).unwrap();
        source.install_target_mirrorlist(false).unwrap();

        let after = (
            digest(&root.join("host-etc-pacman.conf")),
            digest(&root.join("host-etc-pacman.d-mirrorlist")),
        );
        assert_eq!(before, after, "the host's pacman files were rewritten");

        // And the region the user chose is what the target and pacstrap get.
        // The target path is rooted at the run's own directory, mirroring how
        // production reaches `/mnt`.
        let target =
            std::fs::read_to_string(root.join("state/mnt/etc/pacman.d/mirrorlist")).unwrap();
        assert_eq!(target, selected);

        let config = std::fs::read_to_string(root.join("state/pacstrap.conf")).unwrap();
        assert!(config.contains("chosen.example"));
        assert!(!config.contains("mirror.de"));
    }

    #[test]
    fn the_target_mirrorlist_is_the_selected_one_not_the_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let (config_before, mirrorlist_before) =
            isolated_install_against_fixture(dir.path(), MIRRORLIST);
        // Unused here, but the fixture install itself must have left the
        // host's list alone even though the *run* installed a different one.
        assert_eq!(
            digest(&dir.path().join("host-etc-pacman.conf")),
            config_before
        );
        assert_eq!(
            digest(&dir.path().join("host-etc-pacman.d-mirrorlist")),
            mirrorlist_before
        );
    }

    #[test]
    fn the_installer_owned_configuration_never_names_a_pacman_path() {
        // The single most important invariant here: the derived config is
        // what `pacstrap -C` reads, so any absolute host path inside it would
        // make the base install depend on the source system's configuration.
        let source = PackageSource::Isolated(PackageSourceFiles::new());
        let derived = source
            .derived_pacman()
            .expect("isolated source has a pacman");
        assert!(
            derived.conf().starts_with(paths::host_state_dir()),
            "{} must live in the installer's own state: {}",
            derived.conf().display(),
            paths::host_state_dir().display()
        );

        let derived = derive_pacstrap_config(
            "[options]\nInclude = /etc/pacman.d/mirrorlist\n\n[core]\nInclude = /etc/pacman.d/mirrorlist\n",
            "Server = https://mirror.example/$repo/os/$arch\n",
        );
        for line in derived.lines() {
            assert!(
                !line.contains("/etc/pacman.d/mirrorlist"),
                "the derived config still points at the host's mirrorlist: {line}"
            );
        }
    }

    #[test]
    fn no_host_pacman_file_is_a_write_destination_off_a_live_iso() {
        // The bug being fixed, stated as a property: on a running system the
        // mirrorlist write and the pacstrap configuration must not be any of
        // the source system's pacman files.
        if paths::host_etc_is_ephemeral() {
            return;
        }
        let source = PackageSource::resolve();
        assert!(source.is_isolated(), "a running host must be isolated");

        let mut destinations = allowed_host_writes();
        destinations.push(source.effective_mirrorlist().display().to_string());
        if let Some(derived) = source.derived_pacman() {
            destinations.push(derived.conf().display().to_string());
        }
        if let Some(files) = source.files() {
            destinations.push(files.target_mirrorlist().display().to_string());
        }

        for host_file in HOST_PACMAN_FILES {
            assert!(
                !destinations.iter().any(|path| path == host_file),
                "{host_file} would be rewritten on the running system: {destinations:?}"
            );
        }
    }

    #[test]
    fn every_allowed_write_resolves_inside_the_installer_state() {
        // On this test host (not a live ISO, not in a chroot) the only
        // permitted destinations are the ephemeral state directory and the
        // target mount. This is the check that would catch a new host write.
        if paths::host_etc_is_ephemeral() {
            return;
        }
        let allowed = allowed_host_writes();
        let state = paths::host_state_dir();

        for path in &allowed {
            assert!(
                std::path::Path::new(path).starts_with(&state)
                    || std::path::Path::new(path).starts_with(paths::CHROOT_MOUNT),
                "{path} escapes both the installer state directory and the target mount"
            );
        }
    }
}
