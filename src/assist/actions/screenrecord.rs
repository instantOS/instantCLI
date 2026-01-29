//! Screen recording functionality for Wayland (Sway)
//!
//! Uses wf-recorder for capturing and ffmpeg for encoding.
//! Designed for quick recordings to share on GitHub issues, messengers, etc.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::assist::utils::{copy_to_clipboard, show_notification};
use crate::common::display_server::DisplayServer;
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

    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            let proc_path = format!("/proc/{}", pid);
            if std::path::Path::new(&proc_path).exists() {
                return true;
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

fn select_area_wayland() -> Result<String> {
    let output = Command::new("slurp")
        .arg("-d")
        .output()
        .context("Failed to run slurp for area selection")?;

    if !output.status.success() {
        anyhow::bail!("Area selection cancelled");
    }

    let geometry = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if geometry.is_empty() {
        anyhow::bail!("No area selected");
    }

    Ok(geometry)
}

fn generate_recording_filename(format: RecordingFormat) -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    format!("recording_{}.{}", timestamp, format.extension())
}

fn start_recording_impl(geometry: Option<&str>, format: RecordingFormat) -> Result<()> {
    if is_recording() {
        anyhow::bail!("Recording already in progress. Use stop to end it.");
    }

    let display_server = DisplayServer::detect();
    if !display_server.is_wayland() {
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

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let child = cmd.spawn().context("Failed to start wf-recorder")?;

    let pid = child.id();

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
        "Run 'ins assist sr' again to stop recording",
    )?;

    Ok(())
}

pub fn screen_record_area() -> Result<()> {
    if is_recording() {
        return toggle_recording();
    }

    let geometry = select_area_wayland()?;
    start_recording_impl(Some(&geometry), RecordingFormat::Mp4)
}

pub fn screen_record_area_webm() -> Result<()> {
    if is_recording() {
        return toggle_recording();
    }

    let geometry = select_area_wayland()?;
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
            let display_server = DisplayServer::detect();

            copy_to_clipboard(output_path.to_string_lossy().as_bytes(), &display_server)?;

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
