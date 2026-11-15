//! Supervise the installer in a private mount/PID namespace, then release it.
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::resources::cleanup;
use super::{BootstrapContext, MARKER, Request, chroot, run, write_json};
use crate::arch::execution::paths;

fn mount(root: &Path, source: &str, target: &str, options: &[&str]) -> Result<()> {
    let destination = root.join(target.trim_start_matches('/'));
    std::fs::create_dir_all(&destination)?;
    run(Command::new("mount")
        .args(options)
        .arg(source)
        .arg(destination))
}

fn bind_readonly(root: &Path, source: &str, target: &str) -> Result<()> {
    mount(root, source, target, &["--bind"])?;
    run(Command::new("mount")
        .args(["-o", "remount,bind,ro"])
        .arg(root.join(target.trim_start_matches('/'))))
}

/// Private CLI entry point, executed as PID 1 of a new PID/mount namespace.
/// The supervisor remains outside chroot so it can always unmount the target
/// and release resources after the installer exits, fails, or is interrupted.
pub async fn enter(root: &Path) -> Result<()> {
    if nix::unistd::getpid().as_raw() != 1 || !nix::unistd::Uid::effective().is_root() {
        bail!("bootstrap-enter is an internal namespace entry point; use `ins arch install`");
    }
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let root = root.canonicalize()?;
    // /proc is still the parent's procfs here, so /proc/self names this
    // supervisor in the launcher's PID namespace, not its inner PID 1.
    let host_pid: i32 = std::fs::read_link("/proc/self")?
        .to_str()
        .context("Supervisor PID is not UTF-8")?
        .parse()?;
    write_json(&root.join(".ins-supervisor.pid"), &host_pid)?;
    let context: BootstrapContext =
        serde_json::from_slice(&std::fs::read(root.join(MARKER.trim_start_matches('/')))?)?;
    // libalpm's CheckSpace requires a mount entry for the bootstrap root.
    run(Command::new("mount").arg("--bind").arg(&root).arg(&root))?;
    mount(
        &root,
        "proc",
        "/proc",
        &["-t", "proc", "-o", "nosuid,nodev,noexec"],
    )?;
    mount(&root, "/dev", "/dev", &["--rbind"])?;
    // A plain bind omits the host's efivarfs submount. The sysfs tree itself
    // is read-only; firmware boot variables are never exposed writable.
    bind_readonly(&root, "/sys", "/sys")?;
    mount(
        &root,
        "tmpfs",
        "/run",
        &["-t", "tmpfs", "-o", "mode=755,nosuid,nodev"],
    )?;
    mount(
        &root,
        paths::EPHEMERAL_STATE_ROOT,
        paths::EPHEMERAL_STATE_ROOT,
        &["--bind"],
    )?;
    if Path::new("/run/udev").exists() {
        bind_readonly(&root, "/run/udev", "/run/udev")?;
    }
    // modprobe must load modules for the running host kernel, rather than
    // looking for an Arch kernel in the temporary userspace.
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
    let modules = format!("/lib/modules/{}", release.trim());
    if Path::new(&modules).is_dir() {
        let destination = format!("/usr/lib/modules/{}", release.trim());
        bind_readonly(&root, &modules, &destination)?;
    }
    // Both symlink conventions work, including hosts with /dev/shm -> /run/shm.
    std::fs::create_dir_all(root.join("run/shm"))?;
    let result = supervise(&root, &context.request, &mut interrupt, &mut terminate).await;
    // No installer descendants may retain target mounts or race cleanup. PID 1
    // is excluded by kill(-1), and no host process is visible in this namespace.
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(-1),
        nix::sys::signal::Signal::SIGKILL,
    );
    // Wait for kernel resource release before unmounting or closing mappings.
    for _ in 0..500 {
        match nix::sys::wait::waitpid(
            nix::unistd::Pid::from_raw(-1),
            Some(nix::sys::wait::WaitPidFlag::WNOHANG),
        ) {
            Err(nix::errno::Errno::ECHILD) => break,
            Ok(nix::sys::wait::WaitStatus::StillAlive) => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await
            }
            _ => {}
        }
    }
    let cleanup = cleanup(&root);
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => Err(error.context(cleanup.to_string())),
        (result, cleanup) => result.and(cleanup),
    }
}

async fn supervise(
    root: &Path,
    request: &Request,
    interrupt: &mut tokio::signal::unix::Signal,
    terminate: &mut tokio::signal::unix::Signal,
) -> Result<()> {
    let mut command = chroot(root);
    command.args(["/bin/bash", "-eu", "-c", include_str!("prepare.sh")]);
    wait_interruptible(command, interrupt, terminate)
        .await
        .context("Could not prepare Arch's installation tools")?;
    let mut command = chroot(root);
    command.arg("/usr/bin/ins").args(request.arguments());
    wait_interruptible(command, interrupt, terminate).await
}

async fn wait_interruptible(
    command: Command,
    interrupt: &mut tokio::signal::unix::Signal,
    terminate: &mut tokio::signal::unix::Signal,
) -> Result<()> {
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn()?;
    tokio::select! {
        result = child.wait() => {
            let status = result?;
            if !status.success() { bail!("Bootstrap command failed: {status}"); }
            Ok(())
        }
        _ = interrupt.recv() => bail!("Installation interrupted; releasing bootstrap resources"),
        _ = terminate.recv() => bail!("Installation terminated; releasing bootstrap resources"),
    }
}
