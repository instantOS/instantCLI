//! Foreign-host installations run the existing installer in an authenticated
//! Arch userspace. The source OS supplies only the kernel and namespace tools.
//!
//! There are three roots: the host, this disposable bootstrap, and /mnt (the
//! installation target). Only the latter is a target chroot. The marker at the
//! bootstrap root makes that distinction explicit instead of guessing from
//! /proc/1/root, which points outside both chroots.
mod download;
mod namespace;
mod protection;
mod resources;

pub use namespace::enter;
pub use resources::{Resource, ensure_encryption_names_available, record, swap_devices};

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::arch::execution::paths;
use protection::ProtectedDevices;

const MARKER: &str = "/.ins-bootstrap.json";
const RESOURCES: &str = "/.ins-bootstrap-resources.json";
const POWER_ACTION: &str = "/.ins-bootstrap-power-action.json";
// Inherited by target chroot processes solely to select a bootloader policy.
// Disk protection never relies on this environment variable.
pub const PRESERVE_FIRMWARE: &str = "INS_INSTALL_PRESERVE_FIRMWARE";

#[derive(Serialize, Deserialize)]
pub enum PowerAction {
    Reboot,
    Poweroff,
}

/// The finished menu runs in the bootstrap, where the host's service manager
/// is deliberately unavailable. Defer power actions until host-side cleanup.
pub fn request_power_action(action: PowerAction) -> Result<()> {
    write_json(Path::new(POWER_ACTION), &action)
}

/// Publish metadata only after the entire new document has been written.
/// In particular, SIGKILL during a resource registration must leave the
/// previous cleanup journal intact. All callers have a single writer.
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(
        path.parent()
            .context("Bootstrap metadata has no parent directory")?,
    )?;
    serde_json::to_writer(&mut temporary, value)?;
    temporary
        .persist(path)
        .context("Could not publish bootstrap metadata")?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct BootstrapContext {
    protected: ProtectedDevices,
    request: Request,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Install { debug: bool },
    Exec { questions: PathBuf, debug: bool },
}

impl Request {
    fn arguments(&self) -> Vec<std::ffi::OsString> {
        let mut args = vec!["arch".into()];
        match self {
            Self::Install { .. } => args.push("install".into()),
            Self::Exec { questions, .. } => {
                args.extend([
                    "exec".into(),
                    "--questions-file".into(),
                    questions.as_os_str().to_owned(),
                ]);
            }
        }
        if match self {
            Self::Install { debug } | Self::Exec { debug, .. } => *debug,
        } {
            args.push("--debug".into());
        }
        args
    }
}

pub fn is_environment() -> bool {
    Path::new(MARKER).exists()
}

pub fn ensure_available(disk: &str) -> Result<()> {
    if is_environment() {
        let context: BootstrapContext = serde_json::from_slice(&std::fs::read(MARKER)?)
            .context("Invalid bootstrap host protection metadata")?;
        context.protected.ensure_available(disk)?;
    }
    Ok(())
}

pub(super) fn private_directory(path: &Path) -> Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(super) fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("Could not start {command:?}"))?;
    if !status.success() {
        bail!("Command failed ({status}): {command:?}");
    }
    Ok(())
}

fn preflight() -> Result<()> {
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        bail!("Foreign-host installation requires x86_64 Linux");
    }
    let missing: Vec<_> = [
        "gpg",
        "tar",
        "zstd",
        "unshare",
        "mount",
        "umount",
        "mountpoint",
        "chroot",
        "lsblk",
        "findmnt",
    ]
    .into_iter()
    .filter(|tool| which::which(tool).is_err())
    .collect();
    if !missing.is_empty() {
        bail!(
            "Missing bootstrap prerequisites: {}. On Ubuntu/Debian install them with:\n  sudo apt install gnupg tar zstd util-linux mount coreutils",
            missing.join(", ")
        );
    }
    Ok(())
}

/// Called on the original host, after privilege escalation and before any
/// wizard step. Logs, answers, and checkpoints survive teardown in /run/ins-install.
pub async fn launch(mut request: Request) -> Result<()> {
    preflight()?;
    if !nix::unistd::Uid::effective().is_root() {
        bail!("The bootstrap requires root privileges");
    }
    let protected = ProtectedDevices::capture()?;
    paths::ensure_host_state_dir()?;
    // A process-wide advisory lock prevents concurrent runs sharing /mnt,
    // state, or the installer's LVM names. Held across namespace execution.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths::host_state_dir().join("bootstrap.lock"))?;
    let _lock = nix::fcntl::Flock::lock(lock, nix::fcntl::FlockArg::LockExclusiveNonblock)
        .map_err(|(_, error)| {
            anyhow::anyhow!("Another bootstrap installation is running: {error}")
        })?;

    if let Request::Exec { questions, .. } = &mut request {
        let content =
            std::fs::read_to_string(&questions).context("Could not read installation questions")?;
        let context: crate::arch::engine::InstallContext =
            toml::from_str(&content).context("Could not parse installation questions")?;
        let disk = context
            .get_answer(&crate::arch::engine::StepId::Disk)
            .context("The installation questions do not select a target disk")?;
        // Validate source protection before downloading; the complete plan is
        // validated again inside Arch before any disk writes.
        protected.ensure_available(disk)?;
        let staged = paths::host_state_dir().join("imported-questions.toml");
        paths::write_host_file(&staged, &content)?;
        *questions = staged;
    }
    println!(
        "Preparing a temporary Arch installation environment. Host-mounted disks are protected; installation logs remain in {}.",
        paths::host_state_dir().display()
    );
    let directory = tempfile::Builder::new()
        .prefix("ins-arch-bootstrap-")
        .tempdir_in("/var/tmp")?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let root = tokio::select! {
        root = prepare_root(directory.path(), protected, request) => root.context(
            "Could not prepare Arch. If the connection failed, repair networking on the source host and rerun the same installation command."
        )?,
        _ = interrupt.recv() => anyhow::bail!("Bootstrap preparation interrupted"),
        _ = terminate.recv() => anyhow::bail!("Bootstrap preparation terminated"),
    };
    let mut child = tokio::process::Command::new("unshare")
        .args([
            "--mount",
            "--pid",
            "--fork",
            "--kill-child=SIGTERM",
            "--propagation",
            "private",
        ])
        .arg(std::env::current_exe()?)
        .args(["arch", "bootstrap-enter"])
        .arg("--root")
        .arg(&root)
        .spawn()
        .context("Could not launch the private bootstrap namespace")?;
    let status = tokio::select! {
        status = child.wait() => status?,
        _ = interrupt.recv() => interrupt_launcher(&mut child, &root).await?,
        _ = terminate.recv() => interrupt_launcher(&mut child, &root).await?,
    };
    if !status.success() {
        bail!(
            "Arch bootstrap installation failed ({status}). Inspect {} for the installation log and saved answers. If the connection failed, repair networking on the source host and rerun the same installation command.",
            paths::host_state_dir().display()
        );
    }
    let power = root.join(POWER_ACTION.trim_start_matches('/'));
    let action: Option<PowerAction> = if power.exists() {
        Some(serde_json::from_slice(&std::fs::read(power)?)?)
    } else {
        None
    };
    directory
        .close()
        .context("Could not remove the temporary bootstrap")?;
    if let Some(action) = action {
        run(&mut Command::new(match action {
            PowerAction::Reboot => "reboot",
            PowerAction::Poweroff => "poweroff",
        }))?;
    }
    Ok(())
}

async fn prepare_root(
    directory: &Path,
    protected: ProtectedDevices,
    request: Request,
) -> Result<PathBuf> {
    let archive = download::fetch(directory).await?;
    run(Command::new("tar")
        .arg("--extract")
        .arg("--file")
        .arg(archive)
        .arg("--directory")
        .arg(directory)
        .arg("--numeric-owner"))?;
    let root = directory.join("root.x86_64");
    if !root.join("usr/bin/pacman").is_file() {
        bail!("Arch bootstrap has no pacman executable");
    }
    std::fs::copy(std::env::current_exe()?, root.join("usr/bin/ins"))?;
    let context = BootstrapContext { protected, request };
    write_json(&root.join(MARKER.trim_start_matches('/')), &context)?;
    // Dereference Ubuntu's systemd-resolved symlink on the host. Never bind
    // the host /run or its systemd/D-Bus sockets into the bootstrap.
    let resolver = root.join("etc/resolv.conf");
    if resolver.symlink_metadata().is_ok() {
        std::fs::remove_file(&resolver)?;
    }
    std::fs::write(&resolver, std::fs::read("/etc/resolv.conf")?)?;
    Ok(root)
}

async fn interrupt_launcher(
    child: &mut tokio::process::Child,
    root: &Path,
) -> Result<std::process::ExitStatus> {
    // unshare ignores SIGINT/SIGTERM while waiting for its child. Address the
    // supervisor directly and wait for its cleanup before removing its root.
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if let Ok(pid) = std::fs::read_to_string(root.join(".ins-supervisor.pid")) {
            let pid: i32 = pid.trim().parse()?;
            match nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGTERM,
            ) {
                // The supervisor may have completed cleanup between reading
                // its PID and signalling it. Still reap unshare before teardown.
                Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
                Err(error) => return Err(error.into()),
            }
            return Ok(child.wait().await?);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn chroot(root: &Path) -> Command {
    let mut command = Command::new("chroot");
    command.arg(root).arg("/usr/bin/env").arg("-i").args([
        "HOME=/root",
        "PATH=/usr/local/sbin:/usr/local/bin:/usr/bin",
        "LANG=C.UTF-8",
        // The minimal bootstrap may not contain /sbin/init for variant detection.
        "INS_ARCH_LINUX_VARIANT=arch",
        "INS_INSTALL_PRESERVE_FIRMWARE=1",
    ]);
    if let Ok(term) = std::env::var("TERM") {
        command.arg(format!("TERM={term}"));
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_selects_arch_without_requiring_an_installed_init_system() {
        let command = chroot(Path::new("/bootstrap"));
        let args: Vec<_> = command.get_args().collect();
        assert!(args.windows(2).any(|pair| pair == ["/usr/bin/env", "-i"]));
        assert!(args.iter().any(|arg| *arg == "INS_ARCH_LINUX_VARIANT=arch"));
    }
}
