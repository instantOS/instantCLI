//! Pacman operations bound to a specific sysroot and configuration.
//!
//! [`Pacman::current`] refers to the process's system, including the target
//! after chroot re-entry. [`PackageSource`] selects an isolated configuration
//! when installing from a running host whose pacman files must stay untouched.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use super::CommandRunner;
use super::package_source::PackageSource;
use crate::arch::arch_linux_variant::ArchLinuxVariant;

pub const INSTANT_MIRRORLIST: &str = include_str!("../instantmirrorlist");

/// The Arch mirrorlist the `artix-archlinux-support` package installs.
const ARCH_MIRRORLIST: &str = "/etc/pacman.d/mirrorlist-arch";

/// Concurrent pacman download streams for the install. Upstream Arch ships a
/// commented `#ParallelDownloads = 5`; we set 10 because package downloads
/// dominate install time and the home link is rarely the bottleneck.
const PARALLEL_DOWNLOADS: u8 = 10;

/// A pacman installation: the `pacman.conf`, the `mirrorlist` it points at, the
/// `[instant]` list beside it, and the system it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pacman {
    /// The system installation this pacman belongs to. `/` for the running
    /// system, the target's mount point for an install.
    sysroot: PathBuf,
    conf: PathBuf,
    mirrorlist: PathBuf,
    instant_mirrorlist: PathBuf,
    linux_variant: ArchLinuxVariant,
}

impl Pacman {
    /// The pacman this process would talk to: the running system's own.
    pub fn current() -> Result<Self> {
        Ok(Self::for_variant(ArchLinuxVariant::detect()?))
    }

    pub fn for_variant(linux_variant: ArchLinuxVariant) -> Self {
        Self::for_target_variant(
            "/",
            "/etc/pacman.conf",
            "/etc/pacman.d/mirrorlist",
            linux_variant,
        )
    }

    /// Pacman for `sysroot` on an explicitly known variant.
    ///
    /// Used where the caller already resolved the variant (the install plan)
    /// and where a test must not depend on the machine running it.
    pub fn for_target_variant(
        sysroot: impl Into<PathBuf>,
        conf: impl Into<PathBuf>,
        mirrorlist: impl Into<PathBuf>,
        linux_variant: ArchLinuxVariant,
    ) -> Self {
        let conf = conf.into();
        let instant_mirrorlist = conf
            .parent()
            .unwrap_or_else(|| Path::new("/"))
            .join("pacman.d/instantmirrorlist");
        Self {
            sysroot: sysroot.into(),
            conf,
            mirrorlist: mirrorlist.into(),
            instant_mirrorlist,
            linux_variant,
        }
    }

    pub fn conf(&self) -> &Path {
        &self.conf
    }

    pub fn mirrorlist(&self) -> &Path {
        &self.mirrorlist
    }

    /// The Arch repository support mirrorlist inside this pacman's system, if the
    /// `artix-archlinux-support` package installed one.
    fn arch_mirrorlist(&self) -> PathBuf {
        self.sysroot.join(ARCH_MIRRORLIST.trim_start_matches('/'))
    }

    /// The package cache this pacman owns.
    pub fn cache_dir(&self) -> PathBuf {
        self.sysroot.join("var/cache/pacman/pkg")
    }

    /// Empty this sysroot's cache. Pacman requires `--sysroot` for a mounted
    /// guest and reads the guest's configuration from there.
    pub fn clean_cache_command(&self) -> Command {
        let mut cmd = Command::new("pacman");
        cmd.arg("--sysroot").arg(&self.sysroot);
        cmd.args(["-Scc", "--noconfirm"]);
        cmd
    }

    /// Bootstrap dependencies on the live ISO without requiring menus or sudo.
    /// Online ISOs may have no sync databases and an old package keyring.
    /// Offline sessions use only their existing databases and package cache.
    pub(crate) fn install_live_dependencies(
        &self,
        packages: &[&str],
        mode: crate::arch::offline::Mode,
        executor: &dyn CommandRunner,
    ) -> Result<()> {
        if packages.is_empty() {
            return Ok(());
        }

        println!("Preparing live ISO package dependencies...");
        let keyring_name = self.linux_variant.keyring_name();
        let keyring_package = self.linux_variant.keyring_package();
        for args in [
            ["--init"].as_slice(),
            ["--populate", keyring_name].as_slice(),
        ] {
            let mut command = Command::new("pacman-key");
            command.args(args);
            executor
                .run(&mut command)
                .with_context(|| format!("Failed to initialize the {keyring_name} keyring"))?;
        }

        if !mode.is_offline() {
            let mut command = Command::new("pacman");
            command.arg("--config").arg(self.conf()).args([
                "-Sy",
                "--needed",
                "--noconfirm",
                keyring_package,
            ]);
            executor.run(&mut command).with_context(|| {
                format!(
                    "Failed to synchronize live ISO repositories and update the {keyring_name} keyring"
                )
            })?;
            return self.install(packages, executor);
        }

        let mut command = Command::new("pacman");
        command
            .arg("--config")
            .arg(self.conf())
            .args(["-S", "--needed", "--noconfirm"])
            .args(packages);
        executor
            .run(&mut command)
            .context("Failed to install live ISO dependencies from the offline package cache")
    }

    /// Install packages with retries. Mirror refreshes use this pacman's
    /// configuration, so callers on a running host must pass an isolated one.
    pub fn install(&self, packages: &[&str], executor: &dyn CommandRunner) -> Result<()> {
        if packages.is_empty() {
            return Ok(());
        }

        let offline = crate::arch::offline::mode().is_offline();

        let mut attempt = 0;
        // Keep refresh markers separate for each distribution/init variant.
        let keyring_refreshed_path = PathBuf::from(format!(
            "/tmp/ins-{}-keyring-refreshed",
            self.linux_variant.id()
        ));

        // Artix uses its own keyring. When the `artix-archlinux-support`
        // repository support is configured, packages from Arch repositories also need the
        // Arch keyring.
        let linux_variant = self.linux_variant;
        let mut keyrings = vec![linux_variant.keyring_package()];
        if linux_variant.is_artix() && self.arch_mirrorlist().exists() {
            keyrings.push("archlinux-keyring");
        }

        loop {
            attempt += 1;
            if attempt > 10 {
                anyhow::bail!(
                    "Package installation failed after 10 attempts. Please check your internet connection."
                );
            }
            if attempt > 1 {
                println!(
                    "Retry attempt {attempt}/10 for packages: {}",
                    packages.join(" ")
                );
            } else {
                println!("Installing packages: {}", packages.join(" "));
            }

            let mut cmd = Command::new("pacman");
            cmd.arg("--config")
                .arg(self.conf())
                .arg("-S")
                .arg("--noconfirm")
                .arg("--needed")
                .args(packages);

            match executor.run(&mut cmd) {
                Ok(_) => {
                    println!("Successfully installed packages.");
                    return Ok(());
                }
                Err(e) => {
                    println!("Package installation failed: {e}");
                    if !offline {
                        println!("Ensure you are connected to the internet.");
                    }

                    // Check if we should refresh keyring.
                    // Don't refresh a keyring we are currently trying to install.
                    let installing_keyring =
                        keyrings.iter().any(|keyring| packages.contains(keyring));
                    let keyring_already_refreshed = keyring_refreshed_path.exists();

                    if !keyring_already_refreshed && !installing_keyring {
                        let mut refreshed = false;
                        for &keyring in &keyrings {
                            println!("Attempting to refresh {keyring}...");
                            let mut key_cmd = Command::new("pacman");
                            key_cmd.args(["--config", &self.conf().to_string_lossy()]);
                            key_cmd.args(["-Sy", keyring, "--noconfirm"]);

                            match executor.run(&mut key_cmd) {
                                Ok(()) => refreshed = true,
                                Err(e) => {
                                    println!("Warning: Failed to refresh {keyring}: {e}");
                                }
                            }
                        }

                        if refreshed {
                            if let Err(e) = std::fs::File::create(&keyring_refreshed_path) {
                                println!("Warning: Failed to create lock file: {e}");
                            }
                            // Continue immediately after keyring refresh to try original packages again
                            continue;
                        }
                    }

                    if offline {
                        // The bundle is static: apart from the keyring refresh
                        // above, no retry can change the outcome, so fail with
                        // the real cause instead of burning identical attempts.
                        anyhow::bail!(
                            "Package installation failed. The offline bundle does not contain every required package: {}",
                            packages.join(" ")
                        );
                    }

                    println!("Updating mirrors...");
                    if let Some(mut ref_cmd) =
                        self.reflector_command(which::which("reflector").is_ok())
                    {
                        if let Err(e) = executor.run(&mut ref_cmd) {
                            println!("Warning: Reflector failed: {e}");
                        }
                    } else {
                        println!("Keeping the variant mirrorlist; skipping mirror optimization.");
                    }

                    if let Err(e) = self.shuffle_mirrors() {
                        println!("Warning: Failed to shuffle mirrors: {e}");
                    }

                    println!("Updating repositories...");
                    let mut up_cmd = Command::new("pacman");
                    up_cmd.args(["--config", &self.conf().to_string_lossy()]);
                    up_cmd.arg("-Sy");
                    if let Err(e) = executor.run(&mut up_cmd) {
                        println!("Warning: Repo update failed: {e}");
                    }

                    println!("Retrying package installation in 4 seconds...");
                    thread::sleep(Duration::from_secs(4));
                }
            }
        }
    }

    /// Reflector serves Arch repositories only. Never let recovery replace
    /// an Artix mirrorlist, even if the user has installed reflector there.
    fn reflector_command(&self, available: bool) -> Option<Command> {
        if self.linux_variant.is_artix() || !available {
            return None;
        }
        let mut command = Command::new("reflector");
        command
            .args([
                "--latest",
                "40",
                "--protocol",
                "http,https",
                "--sort",
                "rate",
                "--save",
            ])
            .arg(self.mirrorlist());
        Some(command)
    }

    /// Append the `[instant]` repository to `pacman.conf` and write its
    /// mirrorlist.
    ///
    /// `offline_content` is the bundle-shaped mirrorlist content for offline
    /// installs (`crate::arch::offline::instant_mirrorlist_override`): `None`
    /// online keeps existing user-authored lists untouched, `Some` forces the
    /// file so a stale or copied list cannot point at the wrong place.
    pub async fn setup_instant_repo(
        &self,
        dry_run: bool,
        offline_content: Option<&str>,
    ) -> Result<()> {
        // This runs inside the target after chroot re-entry, or on the current
        // system for Arch conversion and Doctor repair. Use this pacman's config.
        let signing_conf = self.conf.clone();
        tokio::task::spawn_blocking(move || {
            super::signing::bootstrap(
                &signing_conf,
                &super::CommandExecutor {
                    dry_run,
                    log_file: None,
                },
            )
        })
        .await??;

        let conf = &self.conf;
        let instant_mirrorlist = &self.instant_mirrorlist;

        if dry_run {
            println!("[DRY RUN] Appending [instant] config to {}", conf.display());
            println!("[DRY RUN] Creating {}", instant_mirrorlist.display());
            return Ok(());
        }

        // Check if already exists to avoid duplication
        // Note: Doctor check does this check before calling fix, but good to have here too.
        let mut section_exists = false;
        match tokio::fs::read_to_string(conf).await {
            Ok(content) => {
                if content.contains("[instant]") {
                    section_exists = true;
                }
            }
            Err(_) => {
                // If file doesn't exist, we might be in trouble, but OpenOptions create(true) will handle it
            }
        }

        if !section_exists {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(conf)
                .await?;

            file.write_all(
                b"\n[instant]\nSigLevel = Optional TrustAll\nInclude = /etc/pacman.d/instantmirrorlist\n",
            )
            .await?;
            println!("Added InstantOS repository to {}", conf.display());
        }

        // Write the mirrorlist: always on a fresh section, and always when an
        // offline install supplies bundle-shaped content that must win over
        // whatever was there before.
        let mirrorlist_exists = tokio::fs::try_exists(instant_mirrorlist)
            .await
            .unwrap_or(false);
        if !section_exists || offline_content.is_some() || !mirrorlist_exists {
            let content = offline_content.unwrap_or(INSTANT_MIRRORLIST);
            tokio::fs::write(instant_mirrorlist, content).await?;
        }

        Ok(())
    }

    /// Enable 32-bit packages: `[multilib]` on Arch, `[lib32]` on Artix.
    pub async fn enable_32bit_repositories(&self, dry_run: bool) -> Result<()> {
        let repository = if self.linux_variant.is_artix() {
            "lib32"
        } else {
            "multilib"
        };

        if dry_run {
            println!(
                "[DRY RUN] Enabling [{repository}] in {}",
                self.conf.display()
            );
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&self.conf).await?;

        if let Some(new_content) = uncomment_repository(&content, repository) {
            tokio::fs::write(&self.conf, new_content).await?;
            println!(
                "Enabled [{repository}] repository in {}",
                self.conf.display()
            );
        } else {
            println!(
                "[{repository}] already enabled or not found in {}",
                self.conf.display()
            );
        }

        Ok(())
    }

    /// Add the Arch repositories Artix needs for instantOS packages.
    ///
    /// Only meaningful on Artix, where `artix-archlinux-support` provides
    /// `/etc/pacman.d/mirrorlist-arch`. Existing `[extra]` or `[multilib]`
    /// sections are left untouched, so this is safe to run repeatedly. On
    /// Arch it is a no-op.
    pub async fn enable_additional_arch_repositories(&self, dry_run: bool) -> Result<()> {
        if !self.linux_variant.is_artix() {
            return Ok(());
        }

        let repositories = self.linux_variant.additional_arch_repositories();
        if dry_run {
            println!(
                "[DRY RUN] Enabling Arch repositories {} in {}",
                format_repository_names(repositories),
                self.conf.display()
            );
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&self.conf).await?;
        if let Some(new_content) = append_missing_arch_repositories(&content, repositories) {
            tokio::fs::write(&self.conf, new_content).await?;
            println!(
                "Enabled Arch repositories {} in {}",
                format_repository_names(repositories),
                self.conf.display()
            );
        } else {
            println!(
                "Arch repositories {} already enabled in {}",
                format_repository_names(repositories),
                self.conf.display()
            );
        }

        Ok(())
    }

    /// Make the Arch repositories usable on Artix.
    ///
    /// Installs `artix-archlinux-support`, enables `[extra]`/`[multilib]`,
    /// populates the Arch keyring and refreshes the databases. Idempotent, so
    /// the Config step and the instantOS setup step can both call it.
    pub async fn enable_arch_repository_support(&self, executor: &dyn CommandRunner) -> Result<()> {
        if !self.linux_variant.is_artix() {
            return Ok(());
        }

        println!("Enabling Arch repository support on Artix...");
        self.install(&["artix-archlinux-support"], executor)?;
        self.enable_additional_arch_repositories(executor.dry_run())
            .await?;

        println!("Populating the Arch Linux keyring...");
        let mut populate = Command::new("pacman-key");
        populate.args(["--populate", "archlinux"]);
        executor
            .run(&mut populate)
            .context("Failed to populate the Arch Linux keyring")?;

        println!("Synchronizing package databases...");
        let mut sync = Command::new("pacman");
        sync.arg("-Sy");
        executor.run(&mut sync)?;

        Ok(())
    }

    /// Apply pacman's `[options]` tuning.
    pub async fn configure_settings(&self, dry_run: bool) -> Result<()> {
        if dry_run {
            println!(
                "[DRY RUN] Configuring pacman settings (Candy, Color, ParallelDownloads) in {}",
                self.conf.display()
            );
            return Ok(());
        }

        match tokio::fs::read_to_string(&self.conf).await {
            Ok(content) => {
                if let Some(new_content) = process_pacman_settings(&content) {
                    tokio::fs::write(&self.conf, new_content).await?;
                    println!("Configured pacman settings in {}", self.conf.display());
                } else {
                    println!(
                        "Pacman settings already configured in {}",
                        self.conf.display()
                    );
                }
            }
            Err(e) => {
                println!("Warning: Could not read {}: {e}", self.conf.display());
            }
        }
        Ok(())
    }

    /// Randomise the order of `Server =` lines in this pacman's mirrorlist.
    ///
    /// Reorders only the file it is given. A missing file is not an error: an
    /// offline bundle may never have written one, and there is nothing to
    /// reorder.
    pub fn shuffle_mirrors(&self) -> Result<()> {
        let path = &self.mirrorlist;
        if !path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut new_lines = Vec::new();
        let mut server_pool = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Server") && trimmed.contains('=') {
                server_pool.push(line.to_string());
            } else {
                if !server_pool.is_empty() {
                    let mut rng = rand::rng();
                    server_pool.shuffle(&mut rng);
                    new_lines.append(&mut server_pool);
                }
                new_lines.push(line.to_string());
            }
        }
        if !server_pool.is_empty() {
            let mut rng = rand::rng();
            server_pool.shuffle(&mut rng);
            new_lines.append(&mut server_pool);
        }

        let output = new_lines.join("\n");
        std::fs::write(path, output + "\n")
            .with_context(|| format!("Failed to write {}", path.display()))?;

        println!("Shuffled mirrors in {}", path.display());
        Ok(())
    }
}

/// Wrapper for the variant's bootstrap command (`pacstrap`/`basestrap`) with
/// retry logic.
pub fn bootstrap_packages(
    mount_point: &str,
    packages: &[&str],
    executor: &dyn CommandRunner,
    linux_variant: ArchLinuxVariant,
) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let offline = crate::arch::offline::mode().is_offline();
    let source = PackageSource::resolve(linux_variant);
    let bootstrap = linux_variant.bootstrap_command();

    let mut attempt = 0;

    loop {
        attempt += 1;
        if attempt > 10 {
            anyhow::bail!(
                "{bootstrap} failed after 10 attempts. Please check your internet connection."
            );
        }
        if attempt > 1 {
            println!("Retry attempt {attempt}/10 for {bootstrap}");
        }

        let mut cmd = Command::new(bootstrap);
        // On a live ISO the bootstrap command uses the host's own
        // configuration and copies the host's mirrorlist into the target,
        // which is how the selected region reaches the target. On a running
        // system that would reconfigure the source OS, so it is pointed at the
        // installer's own configuration and told not to copy the host list;
        // the selected mirrorlist is written into the target afterwards.
        // `-C`/`-M` are supported by both pacstrap and basestrap; only an
        // isolated source uses them.
        if let Some(derived) = source.derived_pacman() {
            cmd.arg("-C").arg(derived.conf());
            cmd.arg("-M");
        }
        cmd.arg(mount_point);
        cmd.args(packages);

        match executor.run(&mut cmd) {
            Ok(_) => {
                // The mirrorlist the run selected now has to exist in the
                // target: `pacstrap -M` deliberately skipped copying it.
                source.install_target_mirrorlist(executor.dry_run())?;

                // The shell bootstrap ran this inside the chroot, where it
                // meant the *target's* cache; ported here it ran host-side and
                // emptied the source system's cache instead while leaving the
                // installed one full. Ask the target's pacman, by sysroot.
                let target = source.target_pacman(linux_variant);
                let mut clean_cmd = target.clean_cache_command();
                // `pacman -Scc` still prompts on some versions; answer twice.
                if let Err(error) = executor.run_with_input(&mut clean_cmd, "y\ny\n") {
                    println!(
                        "Warning: Failed to clean {}: {error}",
                        target.cache_dir().display()
                    );
                }
                return Ok(());
            }
            Err(e) => {
                println!("{bootstrap} failed: {e}");

                if offline {
                    // The bundle is static: no mirror shuffle or refresh can
                    // change the outcome, so fail with the real cause instead
                    // of retrying identical work.
                    anyhow::bail!(
                        "{bootstrap} failed. The offline bundle does not contain every required package."
                    );
                }

                println!("Ensure you are connected to the internet.");

                // Reorder the mirrorlist this run installs from — never the
                // source system's.
                if let Some(pacman) = source.derived_pacman().as_ref()
                    && let Err(e) = pacman.shuffle_mirrors()
                {
                    println!("Warning: Failed to shuffle mirrors: {e}");
                }

                println!("Retrying in 2 seconds...");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

/// Uncomment a commented-out repository section and its following `Include`.
///
/// The shipped pacman.conf carries these for exactly this kind of tool:
///
/// ```text
/// #[multilib]
/// #Include = /etc/pacman.d/mirrorlist
/// ```
fn uncomment_repository(content: &str, repository: &str) -> Option<String> {
    let header = format!("#[{repository}]");
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut changed = false;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line == header.as_str() {
            lines[i] = format!("[{repository}]");
            changed = true;

            if i + 1 < lines.len() {
                let next_line = lines[i + 1].trim();
                if next_line.starts_with("#Include") {
                    lines[i + 1] = next_line.replacen("#", "", 1);
                }
            }
        }
        i += 1;
    }

    if changed {
        Some(lines.join("\n"))
    } else {
        None
    }
}

/// Append the Arch repositories Artix needs, unless they are already active.
///
/// Both sections point at the Arch mirrorlist installed by
/// `artix-archlinux-support`, not at the Artix mirrorlist.
fn format_repository_names(repositories: &[&str]) -> String {
    repositories
        .iter()
        .map(|repository| format!("[{repository}]"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn append_missing_arch_repositories(content: &str, repositories: &[&str]) -> Option<String> {
    let mut result = content.to_string();
    let mut changed = false;

    for repository in repositories {
        if has_active_section(&result, repository) {
            continue;
        }

        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&format!("\n[{repository}]\nInclude = {ARCH_MIRRORLIST}\n"));
        changed = true;
    }

    changed.then_some(result)
}

fn has_active_section(content: &str, repository: &str) -> bool {
    let header = format!("[{repository}]");
    content.lines().any(|line| line.trim() == header.as_str())
}

fn process_pacman_settings(content: &str) -> Option<String> {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut changed = false;
    let mut has_candy = false;
    let mut options_idx = None;
    let mut verbose_pkg_lists_idx = None;

    // First pass: analyze structure
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[options]" {
            options_idx = Some(i);
        } else if trimmed == "ILoveCandy" {
            has_candy = true;
        } else if trimmed == "VerbosePkgLists" {
            verbose_pkg_lists_idx = Some(i);
        }
    }

    // Second pass: modifications
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "#Color" {
            *line = "Color".to_string();
            changed = true;
        } else if trimmed.starts_with("#ParallelDownloads") {
            // Replace the commented default with our preferred value.
            *line = format!("ParallelDownloads = {}", PARALLEL_DOWNLOADS);
            changed = true;
        }
    }

    // Handle ILoveCandy insertion
    // We need to re-calculate indices or just insert if we haven't found it
    if !has_candy {
        if let Some(idx) = verbose_pkg_lists_idx {
            // Insert after VerbosePkgLists
            // Note: indices might have shifted if we modified lines? No, we only modified in place.
            // But we need to be careful if we iterate.
            // Vec::insert shifts elements.
            if idx < lines.len() {
                lines.insert(idx + 1, "ILoveCandy".to_string());
                changed = true;
            }
        } else if let Some(idx) = options_idx
            && idx < lines.len()
        {
            lines.insert(idx + 1, "ILoveCandy".to_string());
            changed = true;
        }
    }

    if changed {
        Some(lines.join("\n"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::arch_linux_variant::ArtixInitSystem;
    use crate::arch::execution::mock::MockRunner;

    #[test]
    fn artix_recovery_never_generates_an_arch_mirrorlist() {
        for init in [
            ArtixInitSystem::OpenRC,
            ArtixInitSystem::Runit,
            ArtixInitSystem::S6,
            ArtixInitSystem::Dinit,
        ] {
            assert!(
                Pacman::for_variant(ArchLinuxVariant::Artix(init))
                    .reflector_command(true)
                    .is_none()
            );
        }
        let arch = Pacman::for_variant(ArchLinuxVariant::Arch);
        assert!(arch.reflector_command(true).is_some());
        assert!(arch.reflector_command(false).is_none());
    }

    fn command_text(command: &Command) -> String {
        std::iter::once(command.get_program().to_string_lossy().into_owned())
            .chain(command.get_args().map(|a| a.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_instant_list_sits_beside_the_configuration() {
        // pacman resolves `Include = /etc/pacman.d/…` relative to pacman.conf,
        // so the [instant] list must follow the conf, not a hardcoded /etc.
        let derived = Pacman::for_target_variant(
            "/mnt",
            "/run/ins-install/bootstrap-pacman.conf",
            "/run/ins-install/bootstrap-mirrorlist",
            ArchLinuxVariant::Arch,
        );
        assert_eq!(
            derived.instant_mirrorlist,
            Path::new("/run/ins-install/pacman.d/instantmirrorlist")
        );

        assert_eq!(
            Pacman::for_variant(ArchLinuxVariant::Arch).instant_mirrorlist,
            Path::new("/etc/pacman.d/instantmirrorlist")
        );
    }

    #[test]
    fn the_cache_clean_aims_at_the_target_never_the_source_system() {
        // Regression: this ran as a bare host-side `pacman -Scc`, which emptied
        // the *source* system's package cache while leaving the installed one
        // full. The target's pacman must be named by sysroot instead.
        let runner = MockRunner::new();
        let target = PackageSource::InPlace.target_pacman(ArchLinuxVariant::Arch);

        let mut cmd = target.clean_cache_command();
        runner.run_with_input(&mut cmd, "y\ny\n").unwrap();

        let text = command_text(&cmd);
        assert!(
            text.contains("--sysroot /mnt"),
            "the clean must be scoped to the target: {text}"
        );
        assert!(
            !text.contains("--sysroot / ") && !text.ends_with("--sysroot /"),
            "the clean must never default to the running root: {text}"
        );
        assert_eq!(target.cache_dir(), Path::new("/mnt/var/cache/pacman/pkg"));
        assert_eq!(runner.commands.borrow().len(), 1, "one command, no others");
        assert!(
            runner.commands.borrow()[0].starts_with(&text),
            "the recorded command is the cache clean: {}",
            runner.commands.borrow()[0]
        );
    }

    #[test]
    fn shuffling_keeps_every_server_and_reorders_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mirrorlist");
        let input = "\
## Arch Linux mirrorlist
## Germany
Server = https://one.example/$repo/os/$arch
Server = https://two.example/$repo/os/$arch
## France
Server = https://three.example/$repo/os/$arch
# a comment
";
        std::fs::write(&path, input).unwrap();

        Pacman::for_target_variant(
            dir.path(),
            dir.path().join("pacman.conf"),
            &path,
            ArchLinuxVariant::Arch,
        )
        .shuffle_mirrors()
        .unwrap();
        let shuffled = std::fs::read_to_string(&path).unwrap();

        // Comments and section headings survive; no server is lost.
        assert!(shuffled.contains("## Arch Linux mirrorlist"));
        assert!(shuffled.contains("## Germany"));
        assert!(shuffled.contains("## France"));
        assert!(shuffled.contains("# a comment"));
        for server in ["one", "two", "three"] {
            assert_eq!(
                shuffled
                    .matches(&format!("https://{server}.example"))
                    .count(),
                1,
                "server {server} must survive exactly once: {shuffled}"
            );
        }
        assert!(shuffled.ends_with('\n'));
    }

    #[test]
    fn shuffling_a_missing_mirrorlist_is_a_no_op() {
        // An offline bundle may never have written one, and there is nothing
        // to reorder.
        let dir = tempfile::tempdir().unwrap();
        Pacman::for_target_variant(
            dir.path(),
            dir.path().join("pacman.conf"),
            dir.path().join("absent"),
            ArchLinuxVariant::Arch,
        )
        .shuffle_mirrors()
        .unwrap();
    }

    #[test]
    fn arch_enables_the_multilib_section() {
        let input = r#"
# Some comments
#[multilib]
#Include = /etc/pacman.d/mirrorlist

#[custom]
#Include = ...
"#;
        let expected = r#"
# Some comments
[multilib]
Include = /etc/pacman.d/mirrorlist

#[custom]
#Include = ...
"#;
        let processed = uncomment_repository(input, "multilib").unwrap();
        assert_eq!(processed.trim(), expected.trim());
    }

    #[test]
    fn artix_enables_the_lib32_section() {
        let input = r#"
[options]
#[lib32]
#Include = /etc/pacman.d/mirrorlist
"#;
        let processed = uncomment_repository(input, "lib32").unwrap();
        assert!(processed.contains("[lib32]"), "{processed}");
        assert!(processed.contains("Include = /etc/pacman.d/mirrorlist"));
        assert!(!processed.contains("#[lib32]"));
        assert!(!processed.contains("[multilib]"));
    }

    #[test]
    fn an_already_enabled_repository_is_left_alone() {
        let input = r#"
[multilib]
Include = /etc/pacman.d/mirrorlist
"#;
        assert_eq!(uncomment_repository(input, "multilib"), None);
    }

    #[test]
    fn arch_repositories_are_appended_once() {
        let input = "\
[options]
Architecture = auto

[system]
Include = /etc/pacman.d/mirrorlist
";
        let processed = append_missing_arch_repositories(
            input,
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC).additional_arch_repositories(),
        )
        .unwrap();

        assert_eq!(processed.matches("[extra]").count(), 1, "{processed}");
        assert_eq!(processed.matches("[multilib]").count(), 1, "{processed}");
        assert_eq!(
            processed
                .matches("Include = /etc/pacman.d/mirrorlist-arch")
                .count(),
            2,
            "{processed}"
        );
        assert!(processed.contains("[system]"));
        assert!(processed.ends_with('\n'));
    }

    #[test]
    fn existing_arch_repositories_are_not_duplicated() {
        let input = "\
[extra]
Include = /etc/pacman.d/mirrorlist-arch

[multilib]
Include = /etc/pacman.d/mirrorlist-arch
";
        assert_eq!(
            append_missing_arch_repositories(
                input,
                ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC).additional_arch_repositories(),
            ),
            None
        );
    }

    #[test]
    fn only_the_missing_arch_repository_is_appended() {
        let input = "[extra]\nInclude = /etc/pacman.d/mirrorlist-arch\n";
        let processed = append_missing_arch_repositories(
            input,
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC).additional_arch_repositories(),
        )
        .unwrap();

        assert_eq!(processed.matches("[extra]").count(), 1, "{processed}");
        assert_eq!(processed.matches("[multilib]").count(), 1, "{processed}");
    }

    #[test]
    fn the_pacman_carries_its_variant() {
        let pacman = Pacman::for_target_variant(
            "/mnt",
            "/mnt/etc/pacman.conf",
            "/mnt/etc/pacman.d/mirrorlist",
            ArchLinuxVariant::Artix(ArtixInitSystem::Dinit),
        );
        assert_eq!(
            pacman.linux_variant,
            ArchLinuxVariant::Artix(ArtixInitSystem::Dinit)
        );
    }

    #[test]
    fn live_iso_keyring_commands_follow_the_variant() {
        for (linux_variant, keyring_name, keyring_package) in [
            (ArchLinuxVariant::Arch, "archlinux", "archlinux-keyring"),
            (
                ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
                "artix",
                "artix-keyring",
            ),
        ] {
            let runner = MockRunner::new();
            Pacman::for_target_variant(
                "/",
                "/etc/pacman.conf",
                "/etc/pacman.d/mirrorlist",
                linux_variant,
            )
            .install_live_dependencies(&["fzf"], crate::arch::offline::Mode::Online, &runner)
            .unwrap();

            let commands = runner.command_log();
            assert!(
                commands
                    .iter()
                    .any(|command| command == &format!("pacman-key --populate {keyring_name}")),
                "{linux_variant:?} must populate its own keyring: {commands:?}"
            );
            assert!(
                commands
                    .iter()
                    .any(|command| command.contains(keyring_package)),
                "{linux_variant:?} must install its own keyring package: {commands:?}"
            );
        }
    }

    #[tokio::test]
    async fn enable_32bit_repositories_uses_lib32_on_artix() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("pacman.conf");
        std::fs::write(
            &conf,
            "[options]\n#[lib32]\n#Include = /etc/pacman.d/mirrorlist\n",
        )
        .unwrap();

        Pacman::for_target_variant(
            dir.path(),
            &conf,
            dir.path().join("mirrorlist"),
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
        )
        .enable_32bit_repositories(false)
        .await
        .unwrap();

        let content = std::fs::read_to_string(&conf).unwrap();
        assert!(content.contains("[lib32]"), "{content}");
        assert!(content.contains("Include = /etc/pacman.d/mirrorlist"));
        assert!(!content.contains("[multilib]"));
    }

    #[tokio::test]
    async fn artix_repository_support_populates_the_arch_keyring() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("pacman.conf");
        std::fs::write(
            &conf,
            "[options]\nArchitecture = auto\n\n[system]\nInclude = /etc/pacman.d/mirrorlist\n",
        )
        .unwrap();
        let pacman = Pacman::for_target_variant(
            dir.path(),
            &conf,
            dir.path().join("mirrorlist"),
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
        );
        let runner = MockRunner::new();

        pacman
            .enable_arch_repository_support(&runner)
            .await
            .unwrap();

        let log = runner.command_log();
        let install = format!(
            "pacman --config {} -S --noconfirm --needed artix-archlinux-support",
            conf.display()
        );
        assert!(log.iter().any(|command| command == &install), "{log:?}");
        assert!(
            log.iter()
                .any(|command| command == "pacman-key --populate archlinux"),
            "{log:?}"
        );
        assert!(log.iter().any(|command| command == "pacman -Sy"), "{log:?}");

        let conf = std::fs::read_to_string(&conf).unwrap();
        assert!(conf.contains("[extra]"), "{conf}");
        assert!(conf.contains("[multilib]"), "{conf}");
    }

    #[tokio::test]
    async fn enable_arch_repos_is_a_no_op_on_arch() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("pacman.conf");
        let original = "[options]\nArchitecture = auto\n";
        std::fs::write(&conf, original).unwrap();

        Pacman::for_target_variant(
            dir.path(),
            &conf,
            dir.path().join("mirrorlist"),
            ArchLinuxVariant::Arch,
        )
        .enable_additional_arch_repositories(false)
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(&conf).unwrap(), original);
    }

    #[tokio::test]
    async fn enable_additional_arch_repositories_on_artix() {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("pacman.conf");
        std::fs::write(&conf, "[options]\nArchitecture = auto\n").unwrap();

        Pacman::for_target_variant(
            dir.path(),
            &conf,
            dir.path().join("mirrorlist"),
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
        )
        .enable_additional_arch_repositories(false)
        .await
        .unwrap();

        let content = std::fs::read_to_string(&conf).unwrap();
        assert!(content.contains("[extra]"), "{content}");
        assert!(content.contains("[multilib]"), "{content}");
        assert!(content.contains("Include = /etc/pacman.d/mirrorlist-arch"));
    }

    #[test]
    fn test_process_pacman_settings() {
        let input = r#"
[options]
#VerbosePkgLists
#Color
#ParallelDownloads = 5
"#;
        let expected = format!(
            r#"
[options]
ILoveCandy
#VerbosePkgLists
Color
ParallelDownloads = {}
"#,
            PARALLEL_DOWNLOADS
        );

        let processed = process_pacman_settings(input).unwrap();
        assert_eq!(processed.trim(), expected.trim());
    }

    #[test]
    fn test_process_pacman_settings_with_verbose() {
        let input = r#"
[options]
VerbosePkgLists
#Color
"#;
        let expected = r#"
[options]
VerbosePkgLists
ILoveCandy
Color
"#;
        let processed = process_pacman_settings(input).unwrap();
        assert_eq!(processed.trim(), expected.trim());
    }
}
