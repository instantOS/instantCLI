//! Screen recording functionality for Wayland (Sway)
//!
//! Uses wf-recorder for capturing and ffmpeg for encoding.
//! Designed for quick recordings to share on GitHub issues, messengers, etc.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::assist::utils::{AreaSelectionConfig, copy_to_clipboard, show_notification};
use crate::common::audio::{
    AudioSourceInfo, default_source_names, list_audio_source_names, list_audio_sources_short,
    pactl_defaults,
};
use crate::common::paths;
use crate::common::shell::shell_quote;
use crate::menu::client::MenuClient;
use crate::settings::store::{
    SCREEN_RECORD_AUDIO_SOURCES_KEY, SCREEN_RECORD_FRAMERATE_KEY, SettingsStore,
    is_audio_sources_default, parse_audio_source_selection,
};

const MAX_RECORDING_SECONDS: u64 = 300;
const PID_FILE: &str = "wf-recorder.pid";

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

fn load_audio_selection_mode() -> AudioSelectionMode {
    match SettingsStore::load() {
        Ok(store) => {
            let raw = store.optional_string(SCREEN_RECORD_AUDIO_SOURCES_KEY);
            if is_audio_sources_default(&raw) {
                AudioSelectionMode::Defaults
            } else {
                AudioSelectionMode::Explicit(parse_audio_source_selection(raw))
            }
        }
        Err(_) => AudioSelectionMode::Explicit(Vec::new()),
    }
}

fn load_recording_framerate() -> Option<i64> {
    let value = match SettingsStore::load() {
        Ok(store) => store.int(SCREEN_RECORD_FRAMERATE_KEY),
        Err(_) => SCREEN_RECORD_FRAMERATE_KEY.default,
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

fn is_recording() -> bool {
    let pid_file = get_pid_file();
    if !pid_file.exists() {
        return false;
    }

    match read_recording_pid_info(&pid_file) {
        Ok(info) => {
            let proc_path = format!("/proc/{}", info.pid);
            if std::path::Path::new(&proc_path).exists() {
                return true;
            }

            cleanup_recording_state(&pid_file, &info.audio_module_ids);
        }
        Err(_) => {
            let _ = fs::remove_file(&pid_file);
        }
    }

    false
}

#[derive(Debug)]
struct RecordingPidInfo {
    pid: i32,
    output_path: Option<PathBuf>,
    audio_module_ids: Vec<u32>,
}

fn read_recording_pid_info(pid_file: &Path) -> Result<RecordingPidInfo> {
    let content = fs::read_to_string(pid_file).context("Failed to read PID file")?;
    let mut lines = content.lines();

    let pid: i32 = lines
        .next()
        .context("PID file is empty")?
        .trim()
        .parse()
        .context("Invalid PID in file")?;

    let output_path = lines.next().map(|s| PathBuf::from(s.trim()));
    let audio_module_ids = lines.next().map(parse_audio_module_ids).unwrap_or_default();

    Ok(RecordingPidInfo {
        pid,
        output_path,
        audio_module_ids,
    })
}

fn parse_audio_module_ids(line: &str) -> Vec<u32> {
    line.split(',')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn signal_recorder_stop(pid: i32) -> Result<()> {
    Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .context("Failed to send SIGINT to recorder")?;
    Ok(())
}

fn wait_for_process_exit(pid: i32) {
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let proc_path = format!("/proc/{}", pid);
        if !std::path::Path::new(&proc_path).exists() {
            break;
        }
    }
}

fn cleanup_recording_state(pid_file: &Path, audio_module_ids: &[u32]) {
    let _ = fs::remove_file(pid_file);
    unload_audio_modules(audio_module_ids);
}

fn stop_recording() -> Result<Option<PathBuf>> {
    let pid_file = get_pid_file();
    if !pid_file.exists() {
        return Ok(None);
    }

    let info = read_recording_pid_info(&pid_file)?;
    signal_recorder_stop(info.pid)?;
    wait_for_process_exit(info.pid);
    cleanup_recording_state(&pid_file, &info.audio_module_ids);

    Ok(info.output_path)
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

fn parse_geometry_string(geometry: &str) -> Option<(String, String, String)> {
    // Expected format: WxH+X+Y
    let x_idx = geometry.find('x')?;
    let w = &geometry[..x_idx];
    let rest = &geometry[x_idx + 1..];

    // Find where H ends (at first + or -)
    let offset_idx = rest.find(['+', '-'])?;
    let h = &rest[..offset_idx];
    let offsets = &rest[offset_idx..];

    // Find the second sign for Y offset
    let second_sign_idx = offsets[1..].find(['+', '-'])? + 1;
    let x = &offsets[..second_sign_idx];
    let y = &offsets[second_sign_idx..];

    // Return (WxH, X, Y) with leading + stripped from X/Y
    Some((
        format!("{}x{}", w, h),
        x.trim_start_matches('+').to_string(),
        y.trim_start_matches('+').to_string(),
    ))
}

fn build_ffmpeg_args(
    geometry: Option<&str>,
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

    if let Some(geom) = geometry {
        if let Some((size, x, y)) = parse_geometry_string(geom) {
            args.push("-video_size".to_string());
            args.push(size);
            args.push("-i".to_string());
            args.push(format!(":0.0+{},{}", x, y));
        } else {
            // Fallback if parsing fails - try full screen or error?
            // Safer to error or fallback to full screen
            args.push("-i".to_string());
            args.push(":0.0".to_string());
        }
    } else {
        // Fullscreen
        args.push("-i".to_string());
        args.push(":0.0".to_string());
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

fn spawn_ffmpeg(args: &[String], audio_module_ids: &[u32]) -> Result<u32> {
    let mut setsid_cmd = Command::new("setsid");
    setsid_cmd
        .arg("--fork")
        .arg("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Err(err) = setsid_cmd.spawn() {
        unload_audio_modules(audio_module_ids);
        return Err(err).context("Failed to start ffmpeg via setsid");
    }

    std::thread::sleep(std::time::Duration::from_millis(200));

    let pgrep_output = match Command::new("pgrep").arg("-n").arg("ffmpeg").output() {
        Ok(output) => output,
        Err(err) => {
            unload_audio_modules(audio_module_ids);
            return Err(err).context("Failed to find ffmpeg PID");
        }
    };

    if !pgrep_output.status.success() {
        unload_audio_modules(audio_module_ids);
        anyhow::bail!("ffmpeg failed to start");
    }

    let pid_str = String::from_utf8_lossy(&pgrep_output.stdout);
    let pid: u32 = pid_str
        .trim()
        .parse()
        .inspect_err(|_err| {
            unload_audio_modules(audio_module_ids);
        })
        .context("Failed to parse ffmpeg PID")?;

    Ok(pid)
}

fn build_wf_recorder_args(
    geometry: Option<&str>,
    format: RecordingFormat,
    output_path: &Path,
    audio_target: Option<&AudioTarget>,
    framerate: Option<i64>,
) -> Result<Vec<String>> {
    let mut args = Vec::new();

    if let Some(geom) = geometry {
        args.push("-g".to_string());
        args.push(geom.to_string());
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

    args.push("-x".to_string());
    args.push("yuv420p".to_string());

    Ok(args)
}

fn spawn_wf_recorder(args: &[String], audio_module_ids: &[u32]) -> Result<u32> {
    let mut setsid_cmd = Command::new("setsid");
    setsid_cmd
        .arg("--fork")
        .arg("wf-recorder")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Err(err) = setsid_cmd.spawn() {
        unload_audio_modules(audio_module_ids);
        return Err(err).context("Failed to start wf-recorder via setsid");
    }

    std::thread::sleep(std::time::Duration::from_millis(200));

    let pgrep_output = match Command::new("pgrep").arg("-n").arg("wf-recorder").output() {
        Ok(output) => output,
        Err(err) => {
            unload_audio_modules(audio_module_ids);
            return Err(err).context("Failed to find wf-recorder PID");
        }
    };

    if !pgrep_output.status.success() {
        unload_audio_modules(audio_module_ids);
        anyhow::bail!("wf-recorder failed to start");
    }

    let pid_str = String::from_utf8_lossy(&pgrep_output.stdout);
    let pid: u32 = pid_str
        .trim()
        .parse()
        .inspect_err(|_err| {
            unload_audio_modules(audio_module_ids);
        })
        .context("Failed to parse wf-recorder PID")?;

    Ok(pid)
}

fn write_recording_pid_file(pid: u32, output_path: &Path, audio_module_ids: &[u32]) -> Result<()> {
    let pid_content = if audio_module_ids.is_empty() {
        format!("{}\n{}", pid, output_path.display())
    } else {
        let module_list = audio_module_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>()
            .join(",");
        format!("{}\n{}\n{}", pid, output_path.display(), module_list)
    };

    fs::write(get_pid_file(), pid_content).context("Failed to write PID file")
}

fn schedule_recording_timeout(output_path: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(MAX_RECORDING_SECONDS));
        if is_recording() {
            let _ = stop_recording();
            let _ = show_notification(
                "Recording stopped (timeout)",
                &format!(
                    "Max {} seconds reached. Saved to {}",
                    MAX_RECORDING_SECONDS,
                    output_path.display()
                ),
            );
        }
    });
}

fn start_recording_impl(geometry: Option<&str>, format: RecordingFormat) -> Result<()> {
    ensure_recording_support()?;

    let output_path = recording_output_path(format)?;

    let selection_mode = load_audio_selection_mode();
    let audio_target = resolve_audio_target(selection_mode)?;
    let audio_module_ids = audio_target
        .as_ref()
        .map(|target| target.module_ids().to_vec())
        .unwrap_or_default();
    let framerate = load_recording_framerate();

    let config = AreaSelectionConfig::new();
    let pid = if config.display_server().is_wayland() {
        // Check for unsupported Wayland compositors
        use crate::assist::utils::check_screen_recording_support;
        if !check_screen_recording_support() {
            // Error already shown via menu, just return a quiet error
            anyhow::bail!("Screen recording not supported on this compositor");
        }
        let wf_args = build_wf_recorder_args(
            geometry,
            format,
            &output_path,
            audio_target.as_ref(),
            framerate,
        )?;
        spawn_wf_recorder(&wf_args, &audio_module_ids)?
    } else {
        let ffmpeg_args = build_ffmpeg_args(
            geometry,
            format,
            &output_path,
            audio_target.as_ref(),
            framerate,
        )?;
        spawn_ffmpeg(&ffmpeg_args, &audio_module_ids)?
    };

    if let Err(err) = write_recording_pid_file(pid, &output_path, &audio_module_ids) {
        unload_audio_modules(&audio_module_ids);
        return Err(err);
    }

    schedule_recording_timeout(output_path.clone());

    show_notification(
        "Recording started",
        "Press the same key combo again to stop",
    )?;

    Ok(())
}

pub fn screen_record_area() -> Result<()> {
    if is_recording() {
        return toggle_recording();
    }

    let config = AreaSelectionConfig::new();
    let geometry = config.select_area()?;
    start_recording_impl(Some(&geometry), RecordingFormat::Mp4)
}

pub fn screen_record_area_webm() -> Result<()> {
    if is_recording() {
        return toggle_recording();
    }

    let config = AreaSelectionConfig::new();
    let geometry = config.select_area()?;
    start_recording_impl(Some(&geometry), RecordingFormat::WebM)
}

pub fn screen_record_fullscreen() -> Result<()> {
    if is_recording() {
        return toggle_recording();
    }

    start_recording_impl(None, RecordingFormat::Mp4)
}

pub fn toggle_recording() -> Result<()> {
    if is_recording() {
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
    crate::common::terminal::TerminalLauncher::new("yazi")
        .title("Recording")
        .arg(path_str)
        .launch()?;
    Ok(())
}

fn open_terminal_in_directory(parent_dir: &str) -> Result<()> {
    crate::common::terminal::TerminalLauncher::new("bash")
        .title("Recording Directory")
        .args([
            "-c",
            &format!("cd {} && exec bash", shell_quote(parent_dir)),
        ])
        .launch()?;
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

    let client = MenuClient::new();
    let selected = client.chord(post_recording_menu_options())?;
    let action = selected
        .as_deref()
        .and_then(PostRecordingAction::from_key)
        .unwrap_or(PostRecordingAction::Done);

    handle_post_recording_action(action, &path_str, &parent_dir)
}

pub fn stop_recording_action() -> Result<()> {
    if !is_recording() {
        show_notification("No recording", "No screen recording in progress")?;
        return Ok(());
    }

    toggle_recording()
}
