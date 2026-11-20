use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::audio::unload_audio_modules;

/// Written by the detached shell wrapper, which `exec`s into the recorder and
/// therefore keeps the PID it reports. Read back by us immediately after spawn.
const PID_FILE: &str = "ins-record.pid";
/// Written by us: the PID plus everything needed to validate and clean up.
const STATE_FILE: &str = "ins-record.state";
/// The recorder's stderr. Both ffmpeg and wf-recorder put the actual reason
/// for a failure here, so this must survive until the recording is stopped.
const LOG_FILE: &str = "ins-record.log";

/// How long to wait for the wrapper to report a PID before giving up.
const PID_REPORT_TIMEOUT: Duration = Duration::from_secs(5);
/// Grace period between the wrapper reporting a PID and confirming the
/// recorder is still alive. Long enough for `exec` to have failed on a missing
/// binary, short enough to be invisible.
const RECORDER_PROBE_DELAY: Duration = Duration::from_millis(50);
/// Lines of recorder stderr to quote back when a recording produces no output.
const LOG_TAIL_LINES: usize = 12;
fn get_runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

fn get_pid_file() -> PathBuf {
    get_runtime_dir().join(PID_FILE)
}

pub(super) fn get_state_file() -> PathBuf {
    get_runtime_dir().join(STATE_FILE)
}

fn get_log_file() -> PathBuf {
    get_runtime_dir().join(LOG_FILE)
}

pub(super) fn process_is_alive(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, tail)| !tail.starts_with('Z'))
        })
        .unwrap_or(false)
}

pub(super) fn is_recording() -> Result<bool> {
    is_recording_at(&get_state_file())
}

fn is_recording_at(state_file: &Path) -> Result<bool> {
    if !state_file.exists() {
        return Ok(false);
    }
    let state = match read_recorder_state(state_file) {
        Ok(state) => state,
        Err(err) => {
            remove_recorder_files(state_file, LogDisposition::Keep);
            return Err(err);
        }
    };
    if process_is_alive(state.pid) {
        return Ok(true);
    }
    // Surface failures before allowing another recording to overwrite the log.
    stop_recording_at(state_file)?;
    Ok(false)
}

/// Whether the recorder actually wrote a non-empty file.
///
/// A recorder that starts and immediately fails leaves a 0-byte file behind,
/// which looks like success to anything that only checks for existence.
fn recording_produced_output(output_path: &Option<PathBuf>) -> bool {
    output_path
        .as_ref()
        .is_some_and(|path| fs::metadata(path).is_ok_and(|meta| meta.len() > 0))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RecorderState {
    pub(super) pid: u32,
    pub(super) output_path: Option<PathBuf>,
    pub(super) audio_module_ids: Vec<u32>,
}

pub(super) fn read_recorder_state(state_file: &Path) -> Result<RecorderState> {
    let content = fs::read_to_string(state_file).context("Failed to read recorder state file")?;
    decode_recorder_state(&content)
}

fn decode_recorder_state(content: &str) -> Result<RecorderState> {
    let mut lines = content.lines();

    let pid: u32 = lines
        .next()
        .context("Recorder state file is empty")?
        .trim()
        .parse()
        .context("Invalid PID in recorder state file")?;

    let output_path = lines
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from);

    let audio_module_ids = lines.next().map(parse_audio_module_ids).unwrap_or_default();

    Ok(RecorderState {
        pid,
        output_path,
        audio_module_ids,
    })
}

fn encode_recorder_state(state: &RecorderState) -> String {
    let modules = state
        .audio_module_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<String>>()
        .join(",");

    let output_path = state
        .output_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    format!("{}\n{}\n{}\n", state.pid, output_path, modules)
}

fn parse_audio_module_ids(line: &str) -> Vec<u32> {
    line.split(',')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

pub(super) fn signal_recorder_stop(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .context("Failed to send SIGINT to recorder")?;
    anyhow::ensure!(
        status.success() || !process_is_alive(pid),
        "Failed to stop recorder {pid}"
    );
    Ok(())
}

pub(super) fn wait_for_process_exit(pid: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !process_is_alive(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!("Recorder {pid} did not stop within five seconds; recording state retained")
}

/// Whether the recorder log is still worth keeping on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LogDisposition {
    /// Keep the log: it is the only record of why the recording produced
    /// nothing, so it is left behind for the user to read.
    Keep,
    Discard,
}

impl From<bool> for LogDisposition {
    fn from(recording_succeeded: bool) -> Self {
        if recording_succeeded {
            Self::Discard
        } else {
            Self::Keep
        }
    }
}

/// Drop the PID and state files, plus the recorder log once it is no longer
/// needed.
pub(super) fn remove_recorder_files(state_file: &Path, disposition: LogDisposition) {
    let _ = fs::remove_file(state_file.with_file_name(PID_FILE));
    let _ = fs::remove_file(state_file);
    if disposition == LogDisposition::Discard {
        let _ = fs::remove_file(state_file.with_file_name(LOG_FILE));
    }
}

/// Read the last `LOG_TAIL_LINES` non-empty lines from the recorder log.
///
/// ffmpeg and wf-recorder report the actual reason for a failure on stderr —
/// "height not divisible by 2", unknown encoder, unreadable output path.
/// Sending that to `/dev/null` turned every encoder rejection into the useless
/// "recorder failed to start", so it is kept on disk and quoted back here.
fn read_log_tail(log_file: &Path) -> Option<String> {
    let content = fs::read_to_string(log_file).ok()?;
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();

    if lines.is_empty() {
        return None;
    }

    let start = lines.len().saturating_sub(LOG_TAIL_LINES);
    Some(lines[start..].join("\n"))
}

/// Describe why a recording produced no usable output.
fn no_output_error(state: &RecorderState, log_file: &Path) -> anyhow::Error {
    let detail = match read_log_tail(log_file) {
        Some(tail) => format!("\n\nRecorder output:\n{tail}"),
        None => String::new(),
    };

    anyhow::anyhow!(
        "Recording produced no video data.{detail}\n\nThe empty file was left at {}.",
        state
            .output_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "the output path".to_string())
    )
}

pub(super) fn stop_recording() -> Result<Option<PathBuf>> {
    stop_recording_at(&get_state_file())
}

fn stop_recording_at(state_file: &Path) -> Result<Option<PathBuf>> {
    if !state_file.exists() {
        return Ok(None);
    }

    let state = read_recorder_state(state_file)?;
    if process_is_alive(state.pid) {
        signal_recorder_stop(state.pid)?;
        wait_for_process_exit(state.pid)?;
    }
    unload_audio_modules(&state.audio_module_ids);

    // Check the output before discarding state, so a failed recording reports
    // why instead of silently leaving a 0-byte file behind.
    let produced_output = recording_produced_output(&state.output_path);

    remove_recorder_files(state_file, LogDisposition::from(produced_output));

    if !produced_output {
        return Err(no_output_error(
            &state,
            &state_file.with_file_name(LOG_FILE),
        ));
    }

    Ok(state.output_path)
}

/// POSIX sh wrapper that records its own PID and then `exec`s the recorder.
///
/// `exec` replaces the shell's process image without changing the PID, so the
/// value written here is the recorder's real PID for the entire recording.
const RECORDER_WRAPPER: &str = "echo $$ > \"$1\"; shift; exec \"$@\"";

/// Start `program` in its own session and return its real PID.
///
/// `setsid --fork` always forks, so the PID from `spawn()` belongs to a
/// process that exits immediately and is useless for signalling. The previous
/// code worked around that with a fixed 200 ms sleep followed by
/// `pgrep -n <name>`, which matches by *name* system-wide: it could adopt an
/// unrelated recorder and later SIGINT the wrong process, or miss entirely and
/// report a successful start as a failure.
///
/// The wrapper reports its own PID instead, so there is no race and no
/// guessing. `stderr` goes to a log file rather than a pipe: the recorder is
/// long-running and noisy, and an undrained pipe would eventually fill and
/// block it mid-recording.
pub(super) fn spawn_detached_recorder(program: &str, args: &[String]) -> Result<u32> {
    let pid_file = get_pid_file();
    let log_file = get_log_file();

    let _ = fs::remove_file(&pid_file);

    let log_handle = File::create(&log_file).context("Failed to create recorder log file")?;

    let mut cmd = Command::new("setsid");
    cmd.arg("--fork")
        .arg("sh")
        .arg("-c")
        .arg(RECORDER_WRAPPER)
        // `$0` is a placeholder `sh` requires; it is never read.
        .arg("ins-record-wrapper")
        .arg(&pid_file)
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_handle));

    let status = cmd
        .status()
        .with_context(|| format!("Failed to start {program} via setsid"))?;
    anyhow::ensure!(status.success(), "setsid failed to start {program}");

    let pid = wait_for_pid_report(&pid_file)?;

    // Allow exec and early initialization to complete before reporting success.
    // A missing program leaves a PID report but exits immediately.
    std::thread::sleep(RECORDER_PROBE_DELAY);
    if !process_is_alive(pid) {
        let detail = read_log_tail(&log_file).unwrap_or_default();
        anyhow::bail!(
            "{program} exited immediately and never began recording.{}",
            if detail.is_empty() {
                String::new()
            } else {
                format!("\n\nRecorder output:\n{detail}")
            }
        );
    }

    Ok(pid)
}

fn wait_for_pid_report(pid_file: &Path) -> Result<u32> {
    let deadline = Instant::now() + PID_REPORT_TIMEOUT;

    loop {
        if let Some(pid) = read_reported_pid(pid_file) {
            return Ok(pid);
        }

        if Instant::now() >= deadline {
            break;
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    anyhow::bail!("Recorder did not report its process id within {PID_REPORT_TIMEOUT:?}")
}

fn read_reported_pid(pid_file: &Path) -> Option<u32> {
    fs::read_to_string(pid_file)
        .ok()
        .and_then(|content| content.trim().parse().ok())
}

pub(super) fn write_recorder_state(state: &RecorderState) -> Result<()> {
    fs::write(get_state_file(), encode_recorder_state(state))
        .context("Failed to write recorder state file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_state_reports_error_and_allows_next_attempt() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let state_file = dir.path().join(STATE_FILE);
        fs::write(&state_file, "invalid pid\n")?;
        assert!(is_recording_at(&state_file).is_err());
        assert!(!state_file.exists());
        assert!(!is_recording_at(&state_file)?);
        Ok(())
    }

    #[test]
    fn dead_recorder_reports_failure_before_clearing_state() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output_path = dir.path().join("empty.mp4");
        fs::write(&output_path, [])?;
        let state_file = dir.path().join(STATE_FILE);
        let log_file = dir.path().join(LOG_FILE);
        let state = RecorderState {
            pid: u32::MAX,
            output_path: Some(output_path),
            audio_module_ids: vec![],
        };
        fs::write(&state_file, encode_recorder_state(&state))?;
        fs::write(&log_file, "encoder failed\n")?;
        let error = is_recording_at(&state_file).unwrap_err().to_string();
        assert!(error.contains("encoder failed"));
        assert!(!state_file.exists());
        assert!(log_file.exists());
        assert!(!is_recording_at(&state_file)?);
        Ok(())
    }

    #[test]
    fn recorder_state_round_trips() {
        let state = RecorderState {
            pid: 4242,
            output_path: Some(PathBuf::from("/home/user/Videos/recording.mp4")),
            audio_module_ids: vec![11, 22],
        };

        let decoded = decode_recorder_state(&encode_recorder_state(&state)).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn recorder_state_handles_no_audio_modules() {
        let state = RecorderState {
            pid: 7,
            output_path: None,
            audio_module_ids: Vec::new(),
        };

        let encoded = encode_recorder_state(&state);
        let mut lines = encoded.lines();

        assert_eq!(lines.next(), Some("7"));
        assert_eq!(lines.next(), Some(""));
        assert_eq!(lines.next(), Some(""));
    }

    #[test]
    fn parses_audio_module_ids() {
        assert_eq!(parse_audio_module_ids("1,2,3"), vec![1, 2, 3]);
        assert_eq!(parse_audio_module_ids(" 4 , 5 "), vec![4, 5]);
        assert_eq!(parse_audio_module_ids(""), Vec::<u32>::new());
        assert_eq!(parse_audio_module_ids("nope"), Vec::<u32>::new());
    }

    fn start_test_recorder(pid_file: &Path) -> Result<std::process::ChildStdin> {
        let mut cmd = Command::new("setsid");
        cmd.arg("--fork")
            .arg("sh")
            .arg("-c")
            .arg(RECORDER_WRAPPER)
            .arg("test-wrapper")
            .arg(pid_file)
            .arg("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut launcher = cmd.spawn()?;
        // Keep the pipe outside the launcher: wait() closes Child::stdin.
        // On any early return or panic, dropping this pipe gives cat EOF,
        // including when the wrapper fails to report its PID.
        let input = launcher
            .stdin
            .take()
            .context("missing recorder input pipe")?;
        anyhow::ensure!(launcher.wait()?.success(), "recorder launcher failed");

        Ok(input)
    }

    #[test]
    fn detached_recorder_reports_the_real_pid() -> Result<()> {
        // Integration check for the shell wrapper: the PID written must be the
        // PID of the exec'd process, not of `sh` or `setsid`.
        let dir = tempfile::tempdir()?;
        let pid_file = dir.path().join(PID_FILE);

        let _input = start_test_recorder(&pid_file)?;

        let pid = wait_for_pid_report(&pid_file)?;

        let deadline = Instant::now() + PID_REPORT_TIMEOUT;
        let cmdline = loop {
            let cmdline = fs::read_to_string(format!("/proc/{pid}/cmdline"))?;
            if cmdline.starts_with("cat\0") || Instant::now() >= deadline {
                break cmdline;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let alive = process_is_alive(pid);
        signal_recorder_stop(pid)?;
        wait_for_process_exit(pid)?;
        assert!(alive);
        assert!(cmdline.starts_with("cat\0"), "got {cmdline:?}");
        let _ = fs::remove_file(&pid_file);
        Ok(())
    }

    #[test]
    fn detached_recorder_exits_when_a_test_panics() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let pid_file = dir.path().join(PID_FILE);
        let input = start_test_recorder(&pid_file)?;
        let pid = wait_for_pid_report(&pid_file)?;
        let failure = std::panic::catch_unwind(move || {
            let _input = input;
            panic!("simulate a test assertion failing before explicit cleanup");
        });
        // The pipe closed during unwinding. No explicit stop signal is sent.
        wait_for_process_exit(pid)?;
        assert!(failure.is_err());
        Ok(())
    }
}
