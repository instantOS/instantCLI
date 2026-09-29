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

pub const INSTANT_MIRRORLIST: &str = include_str!("../instantmirrorlist");

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
}

impl Pacman {
    /// The pacman this process would talk to: the running system's own.
    pub fn current() -> Self {
        Self::for_target("/", "/etc/pacman.conf", "/etc/pacman.d/mirrorlist")
    }

    /// Pacman for `sysroot`, using `conf` and `mirrorlist`. The `[instant]`
    /// mirrorlist is placed beside `conf` for its relative `Include` path.
    pub fn for_target(
        sysroot: impl Into<PathBuf>,
        conf: impl Into<PathBuf>,
        mirrorlist: impl Into<PathBuf>,
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
        }
    }

    pub fn conf(&self) -> &Path {
        &self.conf
    }

    pub fn mirrorlist(&self) -> &Path {
        &self.mirrorlist
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

    /// Install packages with retries. Mirror refreshes use this pacman's
    /// configuration, so callers on a running host must pass an isolated one.
    pub fn install(&self, packages: &[&str], executor: &dyn CommandRunner) -> Result<()> {
        if packages.is_empty() {
            return Ok(());
        }

        let offline = crate::arch::offline::mode().is_offline();

        let mut attempt = 0;
        // Remember keyring refreshes across retries.
        let keyring_refreshed_path = Path::new("/tmp/instant_arch_keyring_refreshed");

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

                    // Check if we should refresh keyring
                    // Don't refresh if we are currently trying to install the keyring itself
                    let installing_keyring = packages.contains(&"archlinux-keyring");
                    let keyring_already_refreshed = keyring_refreshed_path.exists();

                    if !keyring_already_refreshed && !installing_keyring {
                        println!("Attempting to refresh archlinux-keyring...");
                        let mut key_cmd = Command::new("pacman");
                        key_cmd.args(["--config", &self.conf().to_string_lossy()]);
                        key_cmd.args(["-Sy", "archlinux-keyring", "--noconfirm"]);

                        if let Err(e) = executor.run(&mut key_cmd) {
                            println!("Warning: Failed to refresh keyring: {e}");
                        } else {
                            // Mark as refreshed
                            if let Err(e) = std::fs::File::create(keyring_refreshed_path) {
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

                    // Update mirrors
                    println!("Updating mirrors...");
                    if which::which("reflector").is_ok() {
                        let mut ref_cmd = Command::new("reflector");
                        ref_cmd.args([
                            "--latest",
                            "40",
                            "--protocol",
                            "http,https",
                            "--sort",
                            "rate",
                            "--save",
                            &self.mirrorlist().to_string_lossy(),
                        ]);
                        if let Err(e) = executor.run(&mut ref_cmd) {
                            println!("Warning: Reflector failed: {e}");
                        }
                    } else {
                        // Fallback or other mirror tools?
                        // The bash script used pacman-mirrors (Manjaro specific usually)
                        // We'll stick to reflector or just skip if not present.
                        println!("Reflector not found, skipping mirror optimization.");
                    }

                    if let Err(e) = self.shuffle_mirrors() {
                        println!("Warning: Failed to shuffle mirrors: {e}");
                    }

                    // Update repos
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

    pub async fn enable_multilib(&self, dry_run: bool) -> Result<()> {
        if dry_run {
            println!("[DRY RUN] Enabling [multilib] in {}", self.conf.display());
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&self.conf).await?;

        if let Some(new_content) = enable_multilib_in_string(&content) {
            tokio::fs::write(&self.conf, new_content).await?;
            println!("Enabled [multilib] repository in {}", self.conf.display());
        } else {
            println!(
                "[multilib] already enabled or not found in {}",
                self.conf.display()
            );
        }

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

/// Wrapper for pacstrap with retry logic
pub fn pacstrap(mount_point: &str, packages: &[&str], executor: &dyn CommandRunner) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let offline = crate::arch::offline::mode().is_offline();
    let source = PackageSource::resolve();

    let mut attempt = 0;

    loop {
        attempt += 1;
        if attempt > 10 {
            anyhow::bail!(
                "Pacstrap failed after 10 attempts. Please check your internet connection."
            );
        }
        if attempt > 1 {
            println!("Retry attempt {attempt}/10 for pacstrap");
        }

        let mut cmd = Command::new("pacstrap");
        // On a live ISO `pacstrap` uses the host's own configuration and copies
        // the host's mirrorlist into the target, which is how the selected
        // region reaches the target. On a running system that would reconfigure
        // the source OS, so it is pointed at the installer's own configuration
        // and told not to copy the host list; the selected mirrorlist is written
        // into the target afterwards.
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

                // The shell bootstrap ran this inside `arch-chroot`, where it
                // meant the *target's* cache; ported here it ran host-side and
                // emptied the source system's cache instead while leaving the
                // installed one full. Ask the target's pacman, by sysroot.
                let target = source.target_pacman();
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
                println!("Pacstrap failed: {e}");

                if offline {
                    // The bundle is static: no mirror shuffle or refresh can
                    // change the outcome, so fail with the real cause instead
                    // of retrying identical work.
                    anyhow::bail!(
                        "Pacstrap failed. The offline bundle does not contain every required package."
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

fn enable_multilib_in_string(content: &str) -> Option<String> {
    // We look for #[multilib] and the following #Include
    // Pattern:
    // #[multilib]
    // #Include = ...

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut changed = false;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line == "#[multilib]" {
            // Found commented multilib section
            lines[i] = "[multilib]".to_string();
            changed = true;

            // Check next line for Include
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
    use crate::arch::execution::mock::MockRunner;

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
        let derived = Pacman::for_target(
            "/mnt",
            "/run/ins-install/pacstrap.conf",
            "/run/ins-install/pacstrap-mirrorlist",
        );
        assert_eq!(
            derived.instant_mirrorlist,
            Path::new("/run/ins-install/pacman.d/instantmirrorlist")
        );

        assert_eq!(
            Pacman::current().instant_mirrorlist,
            Path::new("/etc/pacman.d/instantmirrorlist")
        );
    }

    #[test]
    fn the_cache_clean_aims_at_the_target_never_the_source_system() {
        // Regression: this ran as a bare host-side `pacman -Scc`, which emptied
        // the *source* system's package cache while leaving the installed one
        // full. The target's pacman must be named by sysroot instead.
        let runner = MockRunner::new();
        let target = PackageSource::InPlace.target_pacman();

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

        Pacman::for_target(dir.path(), dir.path().join("pacman.conf"), &path)
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
        Pacman::for_target(
            dir.path(),
            dir.path().join("pacman.conf"),
            dir.path().join("absent"),
        )
        .shuffle_mirrors()
        .unwrap();
    }

    #[test]
    fn test_enable_multilib() {
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
        let processed = enable_multilib_in_string(input).unwrap();
        assert_eq!(processed.trim(), expected.trim());
    }

    #[test]
    fn test_already_enabled() {
        let input = r#"
[multilib]
Include = /etc/pacman.d/mirrorlist
"#;
        assert_eq!(enable_multilib_in_string(input), None);
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
        // Note: ILoveCandy inserted after [options] because VerbosePkgLists is commented out
        // Wait, in my logic: if VerbosePkgLists is commented, verbose_pkg_lists_idx is None.
        // So it falls back to options_idx.

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
