//! Tracking for external menu child processes (fzf, yazi, ...).
//!
//! Menu children are spawned from several places in `menu_utils`, but the
//! server must be able to cancel them when the scratchpad becomes invisible.
//! This module owns the shared registry and the RAII wrapper used by spawners.

use anyhow::Result;
use std::process::{Child, Command, ExitStatus, Output};
use std::sync::Mutex;

/// Global registry for tracking active menu processes (fzf, yazi, etc.) that can be cancelled
/// when scratchpad becomes invisible
static ACTIVE_MENU_PROCESSES: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Register a process ID as an active menu process
pub(crate) fn register_menu_process(pid: u32) -> Result<()> {
    let mut processes = ACTIVE_MENU_PROCESSES
        .lock()
        .map_err(|e| anyhow::anyhow!("Failed to acquire process lock: {}", e))?;
    processes.push(pid);
    Ok(())
}

/// Unregister a process ID (called when process completes normally)
pub(crate) fn unregister_menu_process(pid: u32) {
    if let Ok(mut processes) = ACTIVE_MENU_PROCESSES.lock() {
        processes.retain(|&p| p != pid);
    }
}

/// Kill all registered menu processes (called when scratchpad becomes invisible)
/// Uses SIGINT to simulate the same behavior as pressing ESC
pub(crate) fn kill_active_menu_processes() -> Result<usize> {
    let processes = if let Ok(mut procs) = ACTIVE_MENU_PROCESSES.lock() {
        let current = procs.clone();
        procs.clear(); // Clear the list since we're killing them all
        current
    } else {
        return Ok(0); // If we can't lock, just return
    };

    let count = processes.len();
    for pid in processes {
        // Use SIGINT (same as Ctrl+C/ESC) instead of SIGTERM to match normal cancellation behavior
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid as i32),
            nix::sys::signal::Signal::SIGINT,
        );
    }

    Ok(count)
}

/// RAII guard for a spawned menu child (fzf, yazi, ...).
///
/// Registers the PID on spawn and always unregisters on drop, closing the
/// stale-PID leak when `write_all`/`wait` fails with `?`. If still running
/// at drop, the child is killed and reaped to avoid zombies.
pub(crate) struct TrackedChild {
    child: Option<Child>,
    pid: u32,
}

impl TrackedChild {
    pub(crate) fn inner_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("tracked child already taken")
    }

    /// Wait for exit without capturing output (inherit-stdio case, e.g. yazi).
    pub(crate) fn wait(mut self) -> Result<ExitStatus> {
        let mut child = self.child.take().expect("tracked child already taken");
        Ok(child.wait()?)
    }

    /// Wait and capture output. On drop-completion the PID registration is
    /// released; error mapping is the caller's concern.
    pub(crate) fn finish_with_output(mut self) -> Result<Output> {
        let child = self.child.take().expect("tracked child already taken");
        Ok(child.wait_with_output()?)
    }
}

impl Drop for TrackedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        unregister_menu_process(self.pid);
    }
}

/// Spawn `cmd` as configured by the caller and track it for cancellation.
pub(crate) fn tracked_spawn(mut cmd: Command) -> Result<TrackedChild> {
    let child = cmd.spawn()?;
    let pid = child.id();
    register_menu_process(pid)?;
    Ok(TrackedChild {
        child: Some(child),
        pid,
    })
}

/// Spawn with piped stdio, feed `input`, wait and return output.
/// Used by the simple (non-streaming) fzf paths.
pub(crate) fn run_tracked_with_input(mut cmd: Command, input: &[u8]) -> Result<Output> {
    use std::io::Write as _;
    use std::process::Stdio;

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tracked = tracked_spawn(cmd)?;
    if let Some(stdin) = tracked.inner_mut().stdin.as_mut() {
        stdin.write_all(input)?;
    }
    tracked.finish_with_output()
}
