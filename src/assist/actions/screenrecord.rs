//! Screen recording functionality for Wayland (Sway)
//!
//! Uses wf-recorder for capturing and ffmpeg for encoding.
//! Designed for quick recordings to share on GitHub issues, messengers, etc.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::assist::utils::{copy_to_clipboard, show_notification, AreaSelectionConfig};
use crate::common::paths;

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

fn is_recording() -> bool {
    let pid_file = get_pid_file();
    if !pid_file.exists() {
        return false;
    }

    if let Ok(content) = fs::read_to_string(&pid_file) {
        if let Some(first_line) = content.lines().next() {
            if let Ok(pid) = first_line.trim().parse::<i32>() {
                let proc_path = format!("/proc/{}", pid);
                if std::path::Path::new(&proc_path).exists() {
                    return true;
                }
            }
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

    let mut cmd = Command::new("wf-recorder");

    if let Some(geom) = geometry {
        cmd.arg("-g").arg(geom);
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

    setsid_cmd
        .spawn()
        .context("Failed to start wf-recorder via setsid")?;

    // Give wf-recorder a moment to start and get its PID
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Find the actual wf-recorder PID
    let pgrep_output = Command::new("pgrep")
        .arg("-n") // newest
        .arg("wf-recorder")
        .output()
        .context("Failed to find wf-recorder PID")?;

    if !pgrep_output.status.success() {
        anyhow::bail!("wf-recorder failed to start");
    }

    let pid_str = String::from_utf8_lossy(&pgrep_output.stdout);
    let pid: u32 = pid_str
        .trim()
        .parse()
        .context("Failed to parse wf-recorder PID")?;

    let pid_content = format!("{}\n{}", pid, output_path.display());
    fs::write(get_pid_file(), pid_content).context("Failed to write PID file")?;

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
            let config = AreaSelectionConfig::new();

            copy_to_clipboard(output_path.to_string_lossy().as_bytes(), config.display_server())?;

            show_notification(
                "Recording saved",
                &format!("{}\n(path copied to clipboard)", output_path.display()),
            )?;
        } else {
            show_notification("Recording stopped", "No output file found")?;
        }
    } else {
        anyhow::bail!("No recording in progress");
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
