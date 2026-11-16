//! Screen recording actions for Wayland/Sway and X11.
//!
//! Coordinates audio setup, recorder startup, and post-recording actions.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::assist::geometry::Geometry;
use crate::assist::utils::{AreaSelectionConfig, show_notification};
use crate::common::paths;
use crate::settings::store::{SCREEN_RECORD_FRAMERATE_KEY, SettingsStore};

mod audio;
mod backend;
mod indicator;
mod post_recording;
mod recorder;

use audio::{audio_selection_mode, resolve_audio_target};
use backend::{RecordingFormat, build_ffmpeg_args, build_wf_recorder_args};
use post_recording::show_post_recording_menu;
use recorder::{
    LogDisposition, RecorderState, get_state_file, is_recording, remove_recorder_files,
    signal_recorder_stop, spawn_detached_recorder, stop_recording, wait_for_process_exit,
    write_recorder_state,
};

pub(crate) use indicator::run as run_recording_indicator;

const KOOHA_FLATPAK_ID: &str = "io.github.seadve.Kooha";

fn recording_framerate(store: Option<&SettingsStore>) -> Option<i64> {
    let value = match store {
        Some(store) => store.int(SCREEN_RECORD_FRAMERATE_KEY),
        None => SCREEN_RECORD_FRAMERATE_KEY.default,
    };

    if value > 0 { Some(value) } else { None }
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
    let mut audio_target = resolve_audio_target(selection_mode);
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

    let pid = spawn_detached_recorder(program, &args)?;

    let state = RecorderState {
        pid,
        output_path: Some(output_path.clone()),
        audio_module_ids,
    };

    if let Err(err) = write_recorder_state(&state) {
        signal_recorder_stop(pid).ok();
        if let Err(stop_error) = wait_for_process_exit(pid) {
            // Keep audio alive if the recorder is still running. Report both errors.
            if let Some(target) = &mut audio_target {
                target.retain_modules();
            }
            return Err(err.context(format!(
                "Recorder cleanup also failed: {stop_error:#}. Audio modules retained for recorder {pid}: {:?}",
                state.audio_module_ids
            )));
        }
        remove_recorder_files(&get_state_file(), LogDisposition::Keep);
        return Err(err);
    }

    if let Some(target) = &mut audio_target {
        target.retain_modules();
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

fn screen_record_selected_area(format: RecordingFormat) -> Result<()> {
    if is_recording()? {
        return toggle_recording();
    }

    let geometry = AreaSelectionConfig::new().select_area()?;
    start_recording_impl(Some(geometry), format)
}

pub fn screen_record_area() -> Result<()> {
    screen_record_selected_area(RecordingFormat::Mp4)
}

pub fn screen_record_area_webm() -> Result<()> {
    screen_record_selected_area(RecordingFormat::WebM)
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

pub fn stop_recording_action() -> Result<()> {
    if !is_recording()? {
        show_notification("No recording", "No screen recording in progress")?;
        return Ok(());
    }

    toggle_recording()
}
