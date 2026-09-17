use std::io::{IsTerminal, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::Result;
use nix::fcntl::{FcntlArg, OFlag, fcntl};

use super::sync::{SyncReport, sync_game_saves};

/// Presentation policy is resolved once per workflow, independently of optional tools.
pub(super) struct GameUi {
    graphical: bool,
}

impl GameUi {
    #[cfg(test)]
    pub(super) fn terminal() -> Self {
        Self { graphical: false }
    }

    pub(super) fn detect() -> Self {
        Self {
            graphical: gui_enabled(
                std::env::var("INS_GAME_GUI").ok().as_deref(),
                std::io::stdout().is_terminal() || std::io::stderr().is_terminal(),
                ["DISPLAY", "WAYLAND_DISPLAY"]
                    .iter()
                    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty())),
            ),
        }
    }

    pub(super) fn sync(&self, title: &str, delay: Duration) -> Result<SyncReport> {
        let mut dialog = self.graphical.then(|| ProgressDialog::new(title)).flatten();
        if !delay.is_zero() {
            let message = format!(
                "Waiting {} seconds before syncing saves...",
                delay.as_secs()
            );
            println!("{message}");
            if let Some(dialog) = &mut dialog {
                dialog.set_text(&message);
            }
            sleep(delay);
        }
        let result = sync_game_saves(None, false, &mut |message| {
            if let Some(dialog) = &mut dialog {
                dialog.set_text(message);
            }
        });
        if let Some(dialog) = dialog {
            dialog.finish();
        }
        result
    }

    /// Caller retains responsibility for returning/logging the error in CLI mode.
    pub(super) fn error(&self, message: &str) {
        if !self.graphical {
            return;
        }
        let mut command = Command::new("zenity");
        command
            .args([
                "--error",
                "--no-markup",
                "--title=InstantCLI - Game Error",
                "--width=400",
            ])
            .arg(format!("--text={message}"));
        // Bound even error dialogs: an unattended launcher must eventually exit.
        if !run_bounded(&mut command, Duration::from_secs(15)) {
            self.notify("Game workflow failed", message);
        }
    }

    pub(super) fn completed(&self, report: &SyncReport) {
        self.notify("Save sync results", &report.completion_message());
    }

    fn notify(&self, title: &str, message: &str) {
        if self.graphical {
            let mut command = Command::new("notify-send");
            command.args(["--", title, message]);
            run_bounded(&mut command, Duration::from_secs(2));
        }
    }
}

fn gui_enabled(override_value: Option<&str>, terminal: bool, display: bool) -> bool {
    if !display {
        return false;
    }
    match override_value {
        Some(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
        Some(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
        _ => !terminal,
    }
}

fn run_bounded(command: &mut Command, timeout: Duration) -> bool {
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => wait_bounded(&mut child, timeout),
        Err(error) => {
            eprintln!("Optional desktop UI unavailable: {error}");
            false
        }
    }
}

fn wait_bounded(child: &mut Child, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < timeout => sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Owns the child directly: no global registration, locks, or worker threads.
struct ProgressDialog {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl ProgressDialog {
    fn new(title: &str) -> Option<Self> {
        let mut command = Command::new("zenity");
        command
            .args([
                "--progress",
                "--pulsate",
                "--auto-close",
                "--no-cancel",
                "--no-markup",
                "--width=380",
            ])
            .arg(format!("--title={title}"))
            .arg("--text=Syncing saves...");
        match Self::spawn(&mut command) {
            Ok(dialog) => Some(dialog),
            Err(error) => {
                eprintln!("Optional progress dialog unavailable: {error}");
                None
            }
        }
    }

    fn spawn(command: &mut Command) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take();
        // Construct before fallible setup so Drop also handles setup failures.
        let dialog = Self { child, stdin };
        if let Some(stdin) = &dialog.stdin {
            let flags = OFlag::from_bits_truncate(fcntl(stdin, FcntlArg::F_GETFL)?);
            fcntl(stdin, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
        }
        Ok(dialog)
    }

    fn set_text(&mut self, text: &str) {
        // A short single-line write fits Linux PIPE_BUF. Drop stale updates on
        // backpressure rather than ever delaying a backup for presentation.
        let line = progress_line(text);
        if let Some(stdin) = &mut self.stdin {
            match stdin.write(line.as_bytes()) {
                Ok(size) if size == line.len() => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                _ => {
                    self.stdin.take();
                }
            }
        }
    }

    fn finish(mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write(b"100\n");
        }
        wait_bounded(&mut self.child, Duration::from_millis(300));
    }
}

impl Drop for ProgressDialog {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn progress_line(text: &str) -> String {
    let text: String = text
        .chars()
        .take(240)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    // Progress mode ignores --no-markup and applies g_strcompress before
    // parsing Pango markup. Escape both layers, only at this UI boundary.
    let text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "\\\\");
    format!("# {text}\n")
}

/// Always attempt post-exit synchronization, preserving both errors if necessary.
pub(super) fn after_launch(launch: Result<()>, sync: impl FnOnce() -> Result<()>) -> Result<()> {
    match (launch, sync()) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(launch), Err(sync)) => Err(anyhow::anyhow!(
            "Game command failed: {launch:#}\nPost-exit sync also failed: {sync:#}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_policy_is_explicit_and_requires_display() {
        for value in ["0", "false", "FALSE"] {
            assert!(!gui_enabled(Some(value), false, true));
        }
        assert!(!gui_enabled(None, true, true));
        assert!(gui_enabled(None, false, true));
        assert!(gui_enabled(Some("true"), true, true));
        assert!(!gui_enabled(Some("1"), false, false));
    }

    #[test]
    fn progress_protocol_is_one_bounded_line() {
        assert_eq!(
            progress_line("A & <B>\n100\rX"),
            "# A &amp; &lt;B&gt; 100 X\n"
        );
        assert_eq!(progress_line("back\\slash"), "# back\\\\slash\n");
        let line = progress_line(&"🦀".repeat(2000));
        assert!(line.len() < 4096);
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn missing_program_is_optional() {
        assert!(
            ProgressDialog::spawn(&mut Command::new("/nonexistent/instantcli-test-zenity"))
                .is_err()
        );
    }

    #[test]
    fn progress_child_is_reaped_on_drop() {
        let mut command = Command::new("sleep");
        command.arg("60");
        let dialog = ProgressDialog::spawn(&mut command).unwrap();
        let pid = nix::unistd::Pid::from_raw(dialog.child.id() as i32);
        drop(dialog);
        assert_eq!(
            nix::sys::wait::waitpid(pid, None),
            Err(nix::errno::Errno::ECHILD)
        );
    }

    #[test]
    fn progress_handles_eof_and_early_exit() {
        let mut command = Command::new("cat");
        let mut dialog = ProgressDialog::spawn(&mut command).unwrap();
        dialog.set_text("hello");
        dialog.finish();
        let mut dialog = ProgressDialog::spawn(&mut Command::new("true")).unwrap();
        dialog.child.wait().unwrap();
        dialog.set_text("already closed");
        dialog.finish();
    }

    #[test]
    fn stalled_reader_does_not_block_sync() {
        let mut command = Command::new("sleep");
        command.arg("60");
        let mut dialog = ProgressDialog::spawn(&mut command).unwrap();
        for _ in 0..1000 {
            dialog.set_text(&"x".repeat(1000));
        }
        dialog.finish();
    }

    #[test]
    fn failed_launch_still_syncs_and_preserves_both_errors() {
        let mut synced = false;
        let error = after_launch(Err(anyhow::anyhow!("crashed")), || {
            synced = true;
            Err(anyhow::anyhow!("offline"))
        })
        .unwrap_err();
        assert!(synced);
        assert!(error.to_string().contains("crashed"));
        assert!(error.to_string().contains("offline"));
        assert!(after_launch(Err(anyhow::anyhow!("crashed")), || Ok(())).is_err());
        assert!(after_launch(Ok(()), || Ok(())).is_ok());
    }
}
