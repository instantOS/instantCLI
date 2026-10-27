//! Screen recording functionality for Wayland (Sway)
//!
//! Uses wf-recorder for capturing and ffmpeg for encoding.
//! Designed for quick recordings to share on GitHub issues, messengers, etc.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::assist::geometry::Geometry;
use crate::assist::utils::{AreaSelectionConfig, copy_to_clipboard, show_notification};
use crate::common::audio::{
    AudioSourceInfo, default_source_names, list_audio_source_names, list_audio_sources_short,
    pactl_defaults,
};
use crate::common::paths;
use crate::menu::client::HostedMenuClient;
use crate::settings::store::{
    SCREEN_RECORD_AUDIO_SOURCES_KEY, SCREEN_RECORD_FRAMERATE_KEY, SettingsStore,
    is_audio_sources_default, parse_audio_source_selection,
};

/// Written by the detached shell wrapper, which `exec`s into the recorder and
/// therefore keeps the PID it reports. Read back by us immediately after spawn.
const PID_FILE: &str = "ins-record.pid";
/// Written by us: the PID plus everything needed to validate and clean up.
const STATE_FILE: &str = "ins-record.state";
/// The recorder's stderr. Both ffmpeg and wf-recorder put the actual reason
/// for a failure here, so this must survive until the recording is stopped.
const LOG_FILE: &str = "ins-record.log";

/// Pad video frames without changing the capture bounds.
const EVEN_FRAME_FILTER: &str = "pad=ceil(iw/2)*2:ceil(ih/2)*2";

/// How long to wait for the wrapper to report a PID before giving up.
const PID_REPORT_TIMEOUT: Duration = Duration::from_secs(5);
/// Grace period between the wrapper reporting a PID and confirming the
/// recorder is still alive. Long enough for `exec` to have failed on a missing
/// binary, short enough to be invisible.
const RECORDER_PROBE_DELAY: Duration = Duration::from_millis(50);
/// Lines of recorder stderr to quote back when a recording produces no output.
const LOG_TAIL_LINES: usize = 12;
const KOOHA_FLATPAK_ID: &str = "io.github.seadve.Kooha";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordingFormat {
    Mp4,
    WebM,
}

impl std::fmt::Display for RecordingFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordingFormat::Mp4 => write!(f, "mp4"),
            RecordingFormat::WebM => write!(f, "webm"),
        }
    }
}

impl RecordingFormat {
    fn extension(&self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
        }
    }

    fn codec(&self) -> &'static str {
        match self {
            Self::Mp4 => "libx264",
            Self::WebM => "libvpx-vp9",
        }
    }

    fn codec_params(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            Self::Mp4 => vec![("preset", "veryfast"), ("crf", "23")],
            Self::WebM => vec![("crf", "33"), ("b", "0")],
        }
    }
}

fn get_runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

fn get_pid_file() -> PathBuf {
    get_runtime_dir().join(PID_FILE)
}

fn get_state_file() -> PathBuf {
    get_runtime_dir().join(STATE_FILE)
}

fn get_log_file() -> PathBuf {
    get_runtime_dir().join(LOG_FILE)
}

#[derive(Debug)]
struct AudioTarget {
    name: String,
    module_ids: Vec<u32>,
}

impl AudioTarget {
    fn module_ids(&self) -> &[u32] {
        &self.module_ids
    }
}

#[derive(Debug)]
enum AudioSelectionMode {
    Defaults,
    Explicit(Vec<String>),
}

fn audio_selection_mode(store: Option<&SettingsStore>) -> AudioSelectionMode {
    match store {
        Some(store) => {
            let raw = store.optional_string(SCREEN_RECORD_AUDIO_SOURCES_KEY);
            if is_audio_sources_default(&raw) {
                AudioSelectionMode::Defaults
            } else {
                AudioSelectionMode::Explicit(parse_audio_source_selection(raw))
            }
        }
        None => AudioSelectionMode::Explicit(Vec::new()),
    }
}

fn recording_framerate(store: Option<&SettingsStore>) -> Option<i64> {
    let value = match store {
        Some(store) => store.int(SCREEN_RECORD_FRAMERATE_KEY),
        None => SCREEN_RECORD_FRAMERATE_KEY.default,
    };

    if value > 0 { Some(value) } else { None }
}

fn notify_audio_issue(message: &str) {
    let _ = show_notification("Screen Recording", message);
}

fn load_available_audio_sources() -> Option<Vec<AudioSourceInfo>> {
    match list_audio_sources_short() {
        Ok(sources) => Some(sources),
        Err(_) => {
            notify_audio_issue("Unable to list audio sources; recording video only.");
            None
        }
    }
}

fn select_audio_sources(
    selection_mode: AudioSelectionMode,
    source_info: &[AudioSourceInfo],
) -> Option<Vec<String>> {
    match selection_mode {
        AudioSelectionMode::Defaults => match pactl_defaults() {
            Ok(defaults) => Some(default_source_names(&defaults, source_info)),
            Err(_) => {
                notify_audio_issue("Unable to detect default audio sources; recording video only.");
                None
            }
        },
        AudioSelectionMode::Explicit(list) => Some(list),
    }
}

fn build_available_source_set(source_info: &[AudioSourceInfo]) -> HashSet<String> {
    source_info
        .iter()
        .map(|source| source.name.clone())
        .collect()
}

fn filter_unavailable_sources(
    mut sources: Vec<String>,
    available_set: &HashSet<String>,
) -> Vec<String> {
    let mut missing = Vec::new();
    sources.retain(|source| {
        if available_set.contains(source) {
            true
        } else {
            missing.push(source.clone());
            false
        }
    });

    if !missing.is_empty() {
        notify_audio_issue(
            "Some selected audio sources were unavailable; recording available sources only.",
        );
    }

    sources
}

fn primary_audio_target(name: &str) -> AudioTarget {
    AudioTarget {
        name: name.to_string(),
        module_ids: Vec::new(),
    }
}

fn build_audio_target_from_sources(sources: Vec<String>) -> Result<AudioTarget> {
    if sources.len() == 1 {
        return Ok(primary_audio_target(&sources[0]));
    }

    match create_recording_mix(&sources) {
        Ok(target) => Ok(target),
        Err(_) => {
            notify_audio_issue("Unable to combine audio sources; recording first source only.");
            Ok(primary_audio_target(&sources[0]))
        }
    }
}

fn resolve_audio_target(selection_mode: AudioSelectionMode) -> Result<Option<AudioTarget>> {
    let source_info = match load_available_audio_sources() {
        Some(info) => info,
        None => return Ok(None),
    };
    let available_set = build_available_source_set(&source_info);

    let mut sources = match select_audio_sources(selection_mode, &source_info) {
        Some(sources) => sources,
        None => return Ok(None),
    };

    sources = dedup_audio_sources(sources);
    if sources.is_empty() {
        return Ok(None);
    }

    let sources = filter_unavailable_sources(sources, &available_set);

    if sources.is_empty() {
        notify_audio_issue("No selected audio sources available; recording video only.");
        return Ok(None);
    }

    Ok(Some(build_audio_target_from_sources(sources)?))
}

fn dedup_audio_sources(sources: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    sources
        .into_iter()
        .filter(|source| seen.insert(source.clone()))
        .collect()
}

fn wait_for_audio_source(name: &str) -> bool {
    for _ in 0..10 {
        if let Ok(sources) = list_audio_source_names()
            && sources.iter().any(|source| source == name)
        {
            return true;
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    false
}

fn create_recording_mix(sources: &[String]) -> Result<AudioTarget> {
    let sink_name = format!(
        "ins_record_mix_{}",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let mut module_ids = Vec::new();

    // Create null sink
    let sink_arg = format!("sink_name={}", sink_name);
    let props_arg = format!("sink_properties=device.description={}", sink_name);

    let sink_output = Command::new("pactl")
        .args(["load-module", "module-null-sink", &sink_arg, &props_arg])
        .output()
        .context("Failed to load module-null-sink")?;

    if !sink_output.status.success() {
        let stderr = String::from_utf8_lossy(&sink_output.stderr);
        unload_audio_modules(&module_ids);
        anyhow::bail!("Failed to create null sink: {}", stderr.trim());
    }

    let sink_module_id = String::from_utf8_lossy(&sink_output.stdout)
        .trim()
        .parse::<u32>()
        .context("Failed to parse null sink module id")?;
    module_ids.push(sink_module_id);

    // Create loopbacks for each source
    for source in sources {
        let source_arg = format!("source={}", source);
        let sink_arg = format!("sink={}", sink_name);

        let loopback_output = Command::new("pactl")
            .args(["load-module", "module-loopback", &source_arg, &sink_arg])
            .output()
            .context("Failed to load module-loopback")?;

        if !loopback_output.status.success() {
            let stderr = String::from_utf8_lossy(&loopback_output.stderr);
            unload_audio_modules(&module_ids);
            anyhow::bail!("Failed to create loopback: {}", stderr.trim());
        }

        let module_id = String::from_utf8_lossy(&loopback_output.stdout)
            .trim()
            .parse::<u32>()
            .context("Failed to parse loopback module id")?;

        module_ids.push(module_id);
    }

    let monitor_name = format!("{}.monitor", sink_name);
    if !wait_for_audio_source(&monitor_name) {
        unload_audio_modules(&module_ids);
        anyhow::bail!("Mixed audio source was not ready");
    }

    Ok(AudioTarget {
        name: monitor_name,
        module_ids,
    })
}

fn unload_audio_module(module_id: u32) {
    let _ = Command::new("pactl")
        .args(["unload-module", &module_id.to_string()])
        .status();
}

fn unload_audio_modules(module_ids: &[u32]) {
    let mut ids: Vec<u32> = module_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for module_id in ids.into_iter().rev() {
        unload_audio_module(module_id);
    }
}

fn process_is_alive(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, tail)| !tail.starts_with('Z'))
        })
        .unwrap_or(false)
}

fn is_recording() -> Result<bool> {
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
struct RecorderState {
    pid: u32,
    output_path: Option<PathBuf>,
    audio_module_ids: Vec<u32>,
}

fn read_recorder_state(state_file: &Path) -> Result<RecorderState> {
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

fn signal_recorder_stop(pid: u32) -> Result<()> {
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

fn wait_for_process_exit(pid: u32) -> Result<()> {
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
enum LogDisposition {
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
fn remove_recorder_files(state_file: &Path, disposition: LogDisposition) {
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

fn stop_recording() -> Result<Option<PathBuf>> {
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

fn generate_recording_filename(format: RecordingFormat) -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    format!("recording_{}.{}", timestamp, format.extension())
}

fn ensure_recording_support() -> Result<()> {
    let config = AreaSelectionConfig::new();
    if !config.display_server().is_wayland() && !config.display_server().is_x11() {
        anyhow::bail!("Screen recording currently only supports Wayland/Sway and X11");
    }

    Ok(())
}

fn recording_output_path(format: RecordingFormat) -> Result<PathBuf> {
    let filename = generate_recording_filename(format);
    Ok(paths::videos_dir()?.join(&filename))
}

fn build_ffmpeg_args(
    geometry: Option<&Geometry>,
    format: RecordingFormat,
    output_path: &Path,
    audio_target: Option<&AudioTarget>,
    framerate: Option<i64>,
) -> Result<Vec<String>> {
    let mut args = vec![
        "-f".to_string(),
        "x11grab".to_string(),
        "-draw_mouse".to_string(),
        "1".to_string(),
    ];

    match geometry {
        Some(geom) => {
            args.push("-video_size".to_string());
            args.push(format!("{}x{}", geom.width, geom.height));
            args.push("-i".to_string());
            args.push(format!(":0.0+{},{}", geom.x, geom.y));
        }
        // Fullscreen
        None => {
            args.push("-i".to_string());
            args.push(":0.0".to_string());
        }
    }

    // Audio options
    if let Some(target) = audio_target {
        args.push("-f".to_string());
        args.push("pulse".to_string());
        args.push("-i".to_string());
        args.push(target.name.clone());
        args.push("-ac".to_string());
        args.push("2".to_string());
    }

    // Framerate
    if let Some(fps) = framerate {
        args.push("-framerate".to_string());
        args.push(fps.to_string());
    }

    // Output options
    args.push("-c:v".to_string());
    args.push(format.codec().to_string());

    // Mapping codec params
    for (key, value) in format.codec_params() {
        if key == "b" {
            args.push("-b:v".to_string());
            args.push(value.to_string());
        } else {
            args.push(format!("-{}", key));
            args.push(value.to_string());
        }
    }

    // Pad the encoded frame, keeping the selected capture region within screen bounds.
    args.extend(["-vf".to_string(), EVEN_FRAME_FILTER.to_string()]);

    // Pixel format
    args.push("-pix_fmt".to_string());
    args.push("yuv420p".to_string());

    // Overwrite output
    args.push("-y".to_string());

    args.push(
        output_path
            .to_str()
            .context("Invalid path encoding")?
            .to_string(),
    );

    Ok(args)
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
fn spawn_detached_recorder(program: &str, args: &[String]) -> Result<u32> {
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

fn build_wf_recorder_args(
    geometry: Option<&Geometry>,
    format: RecordingFormat,
    output_path: &Path,
    audio_target: Option<&AudioTarget>,
    framerate: Option<i64>,
) -> Result<Vec<String>> {
    let mut args = Vec::new();

    if let Some(geom) = geometry {
        args.push("-g".to_string());
        args.push(geom.to_slurp());
    }

    if let Some(target) = audio_target {
        args.push(format!("--audio={}", target.name));
        if matches!(format, RecordingFormat::WebM) {
            args.push("-C".to_string());
            args.push("libopus".to_string());
        }
    }

    if let Some(fps) = framerate {
        args.push("-r".to_string());
        args.push(fps.to_string());
    }

    args.push("-f".to_string());
    args.push(
        output_path
            .to_str()
            .context("Invalid path encoding")?
            .to_string(),
    );

    args.push("-c".to_string());
    args.push(format.codec().to_string());

    for (key, value) in format.codec_params() {
        args.push("-p".to_string());
        args.push(format!("{}={}", key, value));
    }

    args.extend(["-F".to_string(), EVEN_FRAME_FILTER.to_string()]);

    args.push("-x".to_string());
    args.push("yuv420p".to_string());

    Ok(args)
}

fn write_recorder_state(state: &RecorderState) -> Result<()> {
    fs::write(get_state_file(), encode_recorder_state(state))
        .context("Failed to write recorder state file")
}

mod indicator;

pub(crate) use indicator::run as run_recording_indicator;

fn spawn_recording_indicator(pid: u32) -> Result<()> {
    Command::new("setsid")
        .arg(std::env::current_exe().context("Failed to locate ins executable")?)
        .args(["assist", "recording-indicator", "--pid", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .context("Failed to start recording indicator")?;
    Ok(())
}

fn start_recording_impl(geometry: Option<Geometry>, format: RecordingFormat) -> Result<()> {
    ensure_recording_support()?;

    let output_path = recording_output_path(format)?;

    let config = AreaSelectionConfig::new();
    let is_wayland = config.display_server().is_wayland();

    // Reject unsupported compositors before touching audio. Creating the null
    // sink and loopbacks first would orphan those Pulse modules on this path.
    if is_wayland {
        use crate::assist::utils::check_screen_recording_support;
        if !check_screen_recording_support() {
            // Error already shown via menu, just return a quiet error
            anyhow::bail!("Screen recording not supported on this compositor");
        }
    }

    let settings = SettingsStore::load().ok();
    let selection_mode = audio_selection_mode(settings.as_ref());
    let audio_target = resolve_audio_target(selection_mode)?;
    let audio_module_ids = audio_target
        .as_ref()
        .map(|target| target.module_ids().to_vec())
        .unwrap_or_default();
    let framerate = recording_framerate(settings.as_ref());

    let (program, args) = if is_wayland {
        let args = build_wf_recorder_args(
            geometry.as_ref(),
            format,
            &output_path,
            audio_target.as_ref(),
            framerate,
        )?;
        ("wf-recorder", args)
    } else {
        let args = build_ffmpeg_args(
            geometry.as_ref(),
            format,
            &output_path,
            audio_target.as_ref(),
            framerate,
        )?;
        ("ffmpeg", args)
    };

    let pid = match spawn_detached_recorder(program, &args) {
        Ok(pid) => pid,
        Err(err) => {
            unload_audio_modules(&audio_module_ids);
            return Err(err);
        }
    };

    let state = RecorderState {
        pid,
        output_path: Some(output_path.clone()),
        audio_module_ids,
    };

    if let Err(err) = write_recorder_state(&state) {
        signal_recorder_stop(pid).ok();
        wait_for_process_exit(pid)?;
        unload_audio_modules(&state.audio_module_ids);
        remove_recorder_files(&get_state_file(), LogDisposition::Keep);
        return Err(err);
    }

    if let Err(err) = spawn_recording_indicator(pid) {
        let _ = stop_recording();
        return Err(err);
    }

    show_notification(
        "Recording started",
        "Press the same key combo again to stop",
    )?;

    Ok(())
}

pub fn screen_record_area() -> Result<()> {
    if is_recording()? {
        return toggle_recording();
    }

    let config = AreaSelectionConfig::new();
    let geometry = config.select_area()?;
    start_recording_impl(Some(geometry), RecordingFormat::Mp4)
}

pub fn screen_record_area_webm() -> Result<()> {
    if is_recording()? {
        return toggle_recording();
    }

    let config = AreaSelectionConfig::new();
    let geometry = config.select_area()?;
    start_recording_impl(Some(geometry), RecordingFormat::WebM)
}

pub fn screen_record_fullscreen() -> Result<()> {
    if is_recording()? {
        return toggle_recording();
    }

    start_recording_impl(None, RecordingFormat::Mp4)
}

pub fn open_kooha() -> Result<()> {
    Command::new("flatpak")
        .args(["run", KOOHA_FLATPAK_ID])
        .spawn()
        .context("Failed to launch Kooha screen recorder")?;

    Ok(())
}

pub fn toggle_recording() -> Result<()> {
    if is_recording()? {
        if let Some(output_path) = stop_recording()? {
            show_post_recording_menu(&output_path)?;
        } else {
            show_notification("Recording stopped", "No output file found")?;
        }
    } else {
        anyhow::bail!("No recording in progress");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum PostRecordingAction {
    Play,
    CopyPath,
    ShowInFileManager,
    OpenYazi,
    OpenTerminal,
    Done,
}

impl PostRecordingAction {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "p" => Some(Self::Play),
            "c" => Some(Self::CopyPath),
            "f" => Some(Self::ShowInFileManager),
            "y" => Some(Self::OpenYazi),
            "t" => Some(Self::OpenTerminal),
            "d" => Some(Self::Done),
            _ => None,
        }
    }
}

fn post_recording_menu_options() -> Vec<String> {
    vec![
        "p:Play video".to_string(),
        "c:Copy path to clipboard".to_string(),
        "f:Open in file manager".to_string(),
        "y:Open with yazi".to_string(),
        "t:Open terminal in directory".to_string(),
        "d:Done (close menu)".to_string(),
    ]
}

fn recording_parent_dir(output_path: &Path) -> String {
    output_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string())
}

fn play_recording(path_str: &str) -> Result<()> {
    Command::new("xdg-open")
        .arg(path_str)
        .spawn()
        .context("Failed to open video")?;
    Ok(())
}

fn copy_recording_path(path_str: &str) -> Result<()> {
    let config = AreaSelectionConfig::new();
    copy_to_clipboard(path_str.as_bytes(), config.display_server())?;
    show_notification("Path copied", path_str)?;
    Ok(())
}

fn open_recording_in_file_manager(path_str: &str, parent_dir: &str) -> Result<()> {
    let file_uri = format!("file://{}", path_str);
    let dbus_result = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("['{}']", file_uri),
            "''",
        ])
        .status();

    if dbus_result.is_err() || !dbus_result.unwrap().success() {
        Command::new("xdg-open")
            .arg(parent_dir)
            .spawn()
            .context("Failed to open file manager")?;
    }

    Ok(())
}

fn open_recording_in_yazi(path_str: &str) -> Result<()> {
    instantos_terminal::Launcher::new("yazi")
        .title("Recording")
        .arg(path_str)
        .spawn_detached()?;
    Ok(())
}

fn open_terminal_in_directory(parent_dir: &str) -> Result<()> {
    instantos_terminal::Launcher::default()
        .title("Recording Directory")
        .working_directory(parent_dir)
        .spawn_detached()?;
    Ok(())
}

fn notify_recording_saved(path_str: &str) -> Result<()> {
    show_notification("Recording saved", path_str)
}

fn handle_post_recording_action(
    action: PostRecordingAction,
    path_str: &str,
    parent_dir: &str,
) -> Result<()> {
    match action {
        PostRecordingAction::Play => play_recording(path_str),
        PostRecordingAction::CopyPath => copy_recording_path(path_str),
        PostRecordingAction::ShowInFileManager => {
            open_recording_in_file_manager(path_str, parent_dir)
        }
        PostRecordingAction::OpenYazi => open_recording_in_yazi(path_str),
        PostRecordingAction::OpenTerminal => open_terminal_in_directory(parent_dir),
        PostRecordingAction::Done => notify_recording_saved(path_str),
    }
}

fn show_post_recording_menu(output_path: &std::path::Path) -> Result<()> {
    let path_str = output_path.to_string_lossy().to_string();
    let parent_dir = recording_parent_dir(output_path);

    let client = HostedMenuClient::new();
    let selected = client.chord(post_recording_menu_options())?;
    let action = match selected {
        crate::menu_utils::DialogOutcome::Submitted(key) => {
            PostRecordingAction::from_key(&key).unwrap_or(PostRecordingAction::Done)
        }
        crate::menu_utils::DialogOutcome::Cancelled => PostRecordingAction::Done,
    };

    handle_post_recording_action(action, &path_str, &parent_dir)
}

pub fn stop_recording_action() -> Result<()> {
    if !is_recording()? {
        show_notification("No recording", "No screen recording in progress")?;
        return Ok(());
    }

    toggle_recording()
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
    fn odd_area_is_padded_without_expanding_capture_bounds() {
        let geometry = Geometry {
            x: 1,
            y: 0,
            width: 1919,
            height: 1079,
        };
        let wf = build_wf_recorder_args(
            Some(&geometry),
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(wf.windows(2).any(|pair| pair == ["-g", "1,0 1919x1079"]));
        assert!(wf.windows(2).any(|pair| pair == ["-F", EVEN_FRAME_FILTER]));
        let ff = build_ffmpeg_args(
            Some(&geometry),
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(
            ff.windows(2)
                .any(|pair| pair == ["-video_size", "1919x1079"])
        );
        assert!(ff.windows(2).any(|pair| pair == ["-vf", EVEN_FRAME_FILTER]));
    }

    #[test]
    fn fullscreen_recording_has_no_geometry_args() {
        let wf = build_wf_recorder_args(
            None,
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(!wf.contains(&"-g".to_string()));

        let ff = build_ffmpeg_args(
            None,
            RecordingFormat::Mp4,
            Path::new("/tmp/out.mp4"),
            None,
            None,
        )
        .unwrap();
        assert!(!ff.contains(&"-video_size".to_string()));
        assert!(ff.contains(&":0.0".to_string()));
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

    #[test]
    fn detached_recorder_reports_the_real_pid() -> Result<()> {
        // Integration check for the shell wrapper: the PID written must be the
        // PID of the exec'd process, not of `sh` or `setsid`.
        let dir = tempfile::tempdir()?;
        let pid_file = dir.path().join(PID_FILE);

        let mut cmd = Command::new("setsid");
        cmd.arg("--fork")
            .arg("sh")
            .arg("-c")
            .arg(RECORDER_WRAPPER)
            .arg("test-wrapper")
            .arg(&pid_file)
            .arg("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        assert!(cmd.status()?.success());

        let pid = wait_for_pid_report(&pid_file).expect("wrapper should report a PID");

        let deadline = Instant::now() + PID_REPORT_TIMEOUT;
        let cmdline = loop {
            let cmdline = fs::read_to_string(format!("/proc/{pid}/cmdline"))?;
            if cmdline.starts_with("sleep\0") || Instant::now() >= deadline {
                break cmdline;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let alive = process_is_alive(pid);
        signal_recorder_stop(pid)?;
        wait_for_process_exit(pid)?;
        assert!(alive);
        assert!(cmdline.starts_with("sleep\0"), "got {cmdline:?}");
        let _ = fs::remove_file(&pid_file);
        Ok(())
    }
}
