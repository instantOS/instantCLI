//! Screen recording functionality for Wayland (Sway)
//!
//! Uses wf-recorder for capturing and ffmpeg for encoding.
//! Designed for quick recordings to share on GitHub issues, messengers, etc.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::assist::utils::{AreaSelectionConfig, copy_to_clipboard, show_notification};
use crate::common::paths;
use crate::common::shell::shell_quote;
use crate::menu::client::MenuClient;
use crate::settings::store::{
    SettingsStore, SCREEN_RECORD_AUDIO_KEY, SCREEN_RECORD_DESKTOP_AUDIO_KEY,
    SCREEN_RECORD_MIC_AUDIO_KEY,
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

#[derive(Debug, Clone, Copy)]
struct AudioSettings {
    enabled: bool,
    desktop: bool,
    microphone: bool,
}

#[derive(Debug)]
enum AudioTarget {
    Default,
    Device { name: String, module_id: Option<u32> },
}

impl AudioTarget {
    fn module_id(&self) -> Option<u32> {
        match self {
            AudioTarget::Device { module_id, .. } => *module_id,
            AudioTarget::Default => None,
        }
    }
}

#[derive(Debug)]
struct PactlDefaults {
    sink: Option<String>,
    source: Option<String>,
}

#[derive(Debug)]
struct CombinedSource {
    name: String,
    module_id: u32,
}

fn load_audio_settings() -> AudioSettings {
    match SettingsStore::load() {
        Ok(store) => AudioSettings {
            enabled: store.bool(SCREEN_RECORD_AUDIO_KEY),
            desktop: store.bool(SCREEN_RECORD_DESKTOP_AUDIO_KEY),
            microphone: store.bool(SCREEN_RECORD_MIC_AUDIO_KEY),
        },
        Err(_) => AudioSettings {
            enabled: SCREEN_RECORD_AUDIO_KEY.default,
            desktop: SCREEN_RECORD_DESKTOP_AUDIO_KEY.default,
            microphone: SCREEN_RECORD_MIC_AUDIO_KEY.default,
        },
    }
}

fn notify_audio_fallback(message: &str) {
    let _ = show_notification("Screen Recording", message);
}

fn resolve_audio_target(settings: AudioSettings) -> Result<Option<AudioTarget>> {
    if !settings.enabled {
        return Ok(None);
    }

    if !settings.desktop && !settings.microphone {
        notify_audio_fallback("Audio enabled but no sources selected; recording video only.");
        return Ok(None);
    }

    let defaults = match pactl_defaults() {
        Ok(defaults) => defaults,
        Err(_) => {
            notify_audio_fallback("Audio source detection failed; using default audio source.");
            return Ok(Some(AudioTarget::Default));
        }
    };

    let sources = match pactl_list_sources() {
        Ok(sources) => sources,
        Err(_) => {
            notify_audio_fallback("Audio source listing failed; using default audio source.");
            return Ok(Some(AudioTarget::Default));
        }
    };

    let mut selected_sources = Vec::new();
    let mut missing_sources = Vec::new();

    if settings.desktop {
        if let Some(source) = desktop_audio_source(&defaults, &sources) {
            selected_sources.push(source);
        } else {
            missing_sources.push("desktop");
        }
    }

    if settings.microphone {
        if let Some(source) = microphone_audio_source(&defaults, &sources) {
            selected_sources.push(source);
        } else {
            missing_sources.push("microphone");
        }
    }

    if !missing_sources.is_empty() {
        notify_audio_fallback("Some audio sources were unavailable; recording available sources only.");
    }

    selected_sources.sort();
    selected_sources.dedup();

    if selected_sources.is_empty() {
        notify_audio_fallback("No matching audio sources found; using default audio source.");
        return Ok(Some(AudioTarget::Default));
    }

    if selected_sources.len() == 1 {
        return Ok(Some(AudioTarget::Device {
            name: selected_sources[0].clone(),
            module_id: None,
        }));
    }

    match create_combined_source(&selected_sources) {
        Ok(combined) => Ok(Some(AudioTarget::Device {
            name: combined.name,
            module_id: Some(combined.module_id),
        })),
        Err(_) => {
            notify_audio_fallback("Unable to combine audio sources; using default audio source.");
            Ok(Some(AudioTarget::Default))
        }
    }
}

fn pactl_defaults() -> Result<PactlDefaults> {
    let output = Command::new("pactl")
        .arg("info")
        .output()
        .context("Failed to run pactl info")?;

    if !output.status.success() {
        anyhow::bail!("pactl info failed");
    }

    let info = String::from_utf8_lossy(&output.stdout);
    let mut defaults = PactlDefaults {
        sink: None,
        source: None,
    };

    for line in info.lines() {
        if let Some(value) = line.strip_prefix("Default Sink:") {
            defaults.sink = Some(value.trim().to_string());
        }
        if let Some(value) = line.strip_prefix("Default Source:") {
            defaults.source = Some(value.trim().to_string());
        }
    }

    Ok(defaults)
}

fn pactl_list_sources() -> Result<Vec<String>> {
    let output = Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .context("Failed to run pactl list sources")?;

    if !output.status.success() {
        anyhow::bail!("pactl list sources failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sources = Vec::new();

    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let _ = parts.next();
        if let Some(name) = parts.next() {
            sources.push(name.to_string());
        }
    }

    Ok(sources)
}

fn desktop_audio_source(defaults: &PactlDefaults, sources: &[String]) -> Option<String> {
    let sink = defaults.sink.as_ref()?;
    let monitor = format!("{}.monitor", sink);

    if sources.iter().any(|source| source == &monitor) {
        return Some(monitor);
    }

    if let Some(source) = defaults.source.as_ref() {
        if source.ends_with(".monitor") && sources.iter().any(|entry| entry == source) {
            return Some(source.clone());
        }
    }

    None
}

fn microphone_audio_source(defaults: &PactlDefaults, sources: &[String]) -> Option<String> {
    let source = defaults.source.as_ref()?;
    if sources.iter().any(|entry| entry == source) {
        Some(source.clone())
    } else {
        None
    }
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
        anyhow::bail!(
            "Failed to create combined audio source: {}",
            stderr.trim()
        );
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

fn stop_recording() -> Result<Option<PathBuf>> {
    let pid_file = get_pid_file();
    if !pid_file.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&pid_file).context("Failed to read PID file")?;
    let mut lines = content.lines();

    let pid: i32 = lines
        .next()
        .context("PID file is empty")?
        .trim()
        .parse()
        .context("Invalid PID in file")?;

    let output_path = lines.next().map(|s| PathBuf::from(s.trim()));
    let module_id = lines
        .next()
        .and_then(|line| line.trim().parse::<u32>().ok());

    Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .context("Failed to send SIGINT to wf-recorder")?;

    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let proc_path = format!("/proc/{}", pid);
        if !std::path::Path::new(&proc_path).exists() {
            break;
        }
    }

    let _ = fs::remove_file(&pid_file);
    if let Some(module_id) = module_id {
        unload_audio_module(module_id);
    }

    Ok(output_path)
}

fn generate_recording_filename(format: RecordingFormat) -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    format!("recording_{}.{}", timestamp, format.extension())
}

fn start_recording_impl(geometry: Option<&str>, format: RecordingFormat) -> Result<()> {
    let config = AreaSelectionConfig::new();
    if !config.display_server().is_wayland() {
        anyhow::bail!("Screen recording currently only supports Wayland/Sway");
    }

    let filename = generate_recording_filename(format);
    let output_path = paths::videos_dir()?.join(&filename);

    let audio_settings = load_audio_settings();
    let audio_target = resolve_audio_target(audio_settings)?;
    let audio_module_id = audio_target.as_ref().and_then(|target| target.module_id());

    let mut cmd = Command::new("wf-recorder");

    if let Some(geom) = geometry {
        cmd.arg("-g").arg(geom);
    }

    if let Some(target) = &audio_target {
        match target {
            AudioTarget::Default => {
                cmd.arg("-a");
            }
            AudioTarget::Device { name, .. } => {
                cmd.arg(format!("--audio={}", name));
            }
        }
    }

    cmd.arg("-f")
        .arg(output_path.to_str().context("Invalid path encoding")?);

    cmd.arg("-c").arg(format.codec());

    for (key, value) in format.codec_params() {
        cmd.arg("-p").arg(format!("{}={}", key, value));
    }

    cmd.arg("-x").arg("yuv420p");

    // Build the full command string for setsid
    let wf_args: Vec<String> = cmd
        .get_args()
        .map(|s| s.to_string_lossy().to_string())
        .collect();

    let mut setsid_cmd = Command::new("setsid");
    setsid_cmd
        .arg("--fork")
        .arg("wf-recorder")
        .args(&wf_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Err(err) = setsid_cmd.spawn() {
        if let Some(module_id) = audio_module_id {
            unload_audio_module(module_id);
        }
        return Err(err).context("Failed to start wf-recorder via setsid");
    }

    // Give wf-recorder a moment to start and get its PID
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Find the actual wf-recorder PID
    let pgrep_output = match Command::new("pgrep")
        .arg("-n") // newest
        .arg("wf-recorder")
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            if let Some(module_id) = audio_module_id {
                unload_audio_module(module_id);
            }
            return Err(err).context("Failed to find wf-recorder PID");
        }
    };

    if !pgrep_output.status.success() {
        if let Some(module_id) = audio_module_id {
            unload_audio_module(module_id);
        }
        anyhow::bail!("wf-recorder failed to start");
    }

    let pid_str = String::from_utf8_lossy(&pgrep_output.stdout);
    let pid: u32 = pid_str
        .trim()
        .parse()
        .map_err(|err| {
            if let Some(module_id) = audio_module_id {
                unload_audio_module(module_id);
            }
            err
        })
        .context("Failed to parse wf-recorder PID")?;

    let pid_content = match audio_module_id {
        Some(module_id) => format!("{}\n{}\n{}", pid, output_path.display(), module_id),
        None => format!("{}\n{}", pid, output_path.display()),
    };
    fs::write(get_pid_file(), pid_content)
        .map_err(|err| {
            if let Some(module_id) = audio_module_id {
                unload_audio_module(module_id);
            }
            err
        })
        .context("Failed to write PID file")?;

    let output_path_clone = output_path.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(MAX_RECORDING_SECONDS));
        if is_recording() {
            let _ = stop_recording();
            let _ = show_notification(
                "Recording stopped (timeout)",
                &format!(
                    "Max {} seconds reached. Saved to {}",
                    MAX_RECORDING_SECONDS,
                    output_path_clone.display()
                ),
            );
        }
    });

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

fn show_post_recording_menu(output_path: &std::path::Path) -> Result<()> {
    let path_str = output_path.to_string_lossy().to_string();
    let parent_dir = output_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let chords = vec![
        "p:Play video".to_string(),
        "c:Copy path to clipboard".to_string(),
        "f:Open in file manager".to_string(),
        "y:Open with yazi".to_string(),
        "t:Open terminal in directory".to_string(),
        "d:Done (close menu)".to_string(),
    ];

    let client = MenuClient::new();
    let selected = client.chord(chords)?;

    match selected.as_deref() {
        Some("p") => {
            // Play the video with xdg-open
            Command::new("xdg-open")
                .arg(&path_str)
                .spawn()
                .context("Failed to open video")?;
        }
        Some("c") => {
            // Copy path to clipboard
            let config = AreaSelectionConfig::new();
            copy_to_clipboard(path_str.as_bytes(), config.display_server())?;
            show_notification("Path copied", &path_str)?;
        }
        Some("f") => {
            // Try D-Bus FileManager1 first, fallback to xdg-open on directory
            let file_uri = format!("file://{}", &path_str);
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
                    .arg(&parent_dir)
                    .spawn()
                    .context("Failed to open file manager")?;
            }
        }
        Some("y") => {
            // Open yazi in a terminal with the file selected
            crate::common::terminal::TerminalLauncher::new("yazi")
                .title("Recording")
                .arg(&path_str)
                .launch()?;
        }
        Some("t") => {
            // Open terminal in the directory
            crate::common::terminal::TerminalLauncher::new("bash")
                .title("Recording Directory")
                .args([
                    "-c",
                    &format!("cd {} && exec bash", shell_quote(&parent_dir)),
                ])
                .launch()?;
        }
        Some("d") | None => {
            // Just show notification and exit
            show_notification("Recording saved", &path_str)?;
        }
        _ => {}
    }

    Ok(())
}

pub fn stop_recording_action() -> Result<()> {
    if !is_recording() {
        show_notification("No recording", "No screen recording in progress")?;
        return Ok(());
    }

    toggle_recording()
}
