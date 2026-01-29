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
    SCREEN_RECORD_AUDIO_SOURCES_KEY, SettingsStore, is_audio_sources_default,
    parse_audio_source_selection,
};

const MAX_RECORDING_SECONDS: u64 = 300;
const PID_FILE: &str = "wf-recorder.pid";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordingFormat {
    Mp4,
    WebM,
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
    module_id: Option<u32>,
}

impl AudioTarget {
    fn module_id(&self) -> Option<u32> {
        self.module_id
    }
}

#[derive(Debug)]
enum AudioSelectionMode {
    Defaults,
    Explicit(Vec<String>),
}

#[derive(Debug)]
struct CombinedSource {
    name: String,
    module_id: u32,
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
        module_id: None,
    }
}

fn build_audio_target_from_sources(sources: Vec<String>) -> Result<AudioTarget> {
    if sources.len() == 1 {
        return Ok(primary_audio_target(&sources[0]));
    }

    match create_combined_source(&sources) {
        Ok(combined) => {
            if !wait_for_audio_source(&combined.name) {
                unload_audio_module(combined.module_id);
                notify_audio_issue(
                    "Combined audio source was not ready; recording first source only.",
                );
                return Ok(primary_audio_target(&sources[0]));
            }

            Ok(AudioTarget {
                name: combined.name,
                module_id: Some(combined.module_id),
            })
        }
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

fn create_combined_source(slaves: &[String]) -> Result<CombinedSource> {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let source_name = format!("ins_record_mix_{}", timestamp);
    let source_arg = format!("source_name={}", source_name);
    let slaves_arg = format!("slaves={}", slaves.join(","));

    let output = Command::new("pactl")
        .args([
            "load-module",
            "module-combine-source",
            &source_arg,
            &slaves_arg,
        ])
        .output()
        .context("Failed to load module-combine-source")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to create combined audio source: {}", stderr.trim());
    }

    let module_id = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .context("Failed to parse pactl module id")?;

    Ok(CombinedSource {
        name: source_name,
        module_id,
    })
}

fn unload_audio_module(module_id: u32) {
    let _ = Command::new("pactl")
        .args(["unload-module", &module_id.to_string()])
        .status();
}

fn unload_audio_module_if_present(module_id: Option<u32>) {
    if let Some(module_id) = module_id {
        unload_audio_module(module_id);
    }
}

fn is_recording() -> bool {
    let pid_file = get_pid_file();
    if !pid_file.exists() {
        return false;
    }

    if let Ok(content) = fs::read_to_string(&pid_file)
        && let Some(first_line) = content.lines().next()
        && let Ok(pid) = first_line.trim().parse::<i32>()
    {
        let proc_path = format!("/proc/{}", pid);
        if std::path::Path::new(&proc_path).exists() {
            return true;
        }
    }

    let _ = fs::remove_file(&pid_file);
    false
}

#[derive(Debug)]
struct RecordingPidInfo {
    pid: i32,
    output_path: Option<PathBuf>,
    audio_module_id: Option<u32>,
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
    let audio_module_id = lines
        .next()
        .and_then(|line| line.trim().parse::<u32>().ok());

    Ok(RecordingPidInfo {
        pid,
        output_path,
        audio_module_id,
    })
}

fn signal_wf_recorder_stop(pid: i32) -> Result<()> {
    Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .context("Failed to send SIGINT to wf-recorder")?;
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

fn cleanup_recording_state(pid_file: &Path, audio_module_id: Option<u32>) {
    let _ = fs::remove_file(pid_file);
    unload_audio_module_if_present(audio_module_id);
}

fn stop_recording() -> Result<Option<PathBuf>> {
    let pid_file = get_pid_file();
    if !pid_file.exists() {
        return Ok(None);
    }

    let info = read_recording_pid_info(&pid_file)?;
    signal_wf_recorder_stop(info.pid)?;
    wait_for_process_exit(info.pid);
    cleanup_recording_state(&pid_file, info.audio_module_id);

    Ok(info.output_path)
}

fn generate_recording_filename(format: RecordingFormat) -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    format!("recording_{}.{}", timestamp, format.extension())
}

fn ensure_wayland_recording() -> Result<()> {
    let config = AreaSelectionConfig::new();
    if !config.display_server().is_wayland() {
        anyhow::bail!("Screen recording currently only supports Wayland/Sway");
    }

    Ok(())
}

fn recording_output_path(format: RecordingFormat) -> Result<PathBuf> {
    let filename = generate_recording_filename(format);
    Ok(paths::videos_dir()?.join(&filename))
}

fn build_wf_recorder_args(
    geometry: Option<&str>,
    format: RecordingFormat,
    output_path: &Path,
    audio_target: Option<&AudioTarget>,
) -> Result<Vec<String>> {
    let mut args = Vec::new();

    if let Some(geom) = geometry {
        args.push("-g".to_string());
        args.push(geom.to_string());
    }

    if let Some(target) = audio_target {
        args.push(format!("--audio={}", target.name));
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

fn spawn_wf_recorder(args: &[String], audio_module_id: Option<u32>) -> Result<u32> {
    let mut setsid_cmd = Command::new("setsid");
    setsid_cmd
        .arg("--fork")
        .arg("wf-recorder")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Err(err) = setsid_cmd.spawn() {
        unload_audio_module_if_present(audio_module_id);
        return Err(err).context("Failed to start wf-recorder via setsid");
    }

    std::thread::sleep(std::time::Duration::from_millis(200));

    let pgrep_output = match Command::new("pgrep").arg("-n").arg("wf-recorder").output() {
        Ok(output) => output,
        Err(err) => {
            unload_audio_module_if_present(audio_module_id);
            return Err(err).context("Failed to find wf-recorder PID");
        }
    };

    if !pgrep_output.status.success() {
        unload_audio_module_if_present(audio_module_id);
        anyhow::bail!("wf-recorder failed to start");
    }

    let pid_str = String::from_utf8_lossy(&pgrep_output.stdout);
    let pid: u32 = pid_str
        .trim()
        .parse()
        .inspect_err(|_err| {
            unload_audio_module_if_present(audio_module_id);
        })
        .context("Failed to parse wf-recorder PID")?;

    Ok(pid)
}

fn write_recording_pid_file(
    pid: u32,
    output_path: &Path,
    audio_module_id: Option<u32>,
) -> Result<()> {
    let pid_content = match audio_module_id {
        Some(module_id) => format!("{}\n{}\n{}", pid, output_path.display(), module_id),
        None => format!("{}\n{}", pid, output_path.display()),
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
    ensure_wayland_recording()?;

    let output_path = recording_output_path(format)?;

    let selection_mode = load_audio_selection_mode();
    let audio_target = resolve_audio_target(selection_mode)?;
    let audio_module_id = audio_target.as_ref().and_then(|target| target.module_id());

    let wf_args = build_wf_recorder_args(geometry, format, &output_path, audio_target.as_ref())?;
    let pid = spawn_wf_recorder(&wf_args, audio_module_id)?;

    if let Err(err) = write_recording_pid_file(pid, &output_path, audio_module_id) {
        unload_audio_module_if_present(audio_module_id);
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
