use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use ksni::TrayMethods;

use super::{
    RecordingPidInfo, cleanup_recording_state, get_pid_file, read_recording_pid_info,
    show_notification, stop_recording_action,
};

struct RecordingTray {
    stop: tokio::sync::mpsc::UnboundedSender<()>,
    output_path: PathBuf,
}

impl ksni::Tray for RecordingTray {
    fn id(&self) -> String {
        "ins-screen-recording".into()
    }

    fn title(&self) -> String {
        "Screen recording in progress".into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.stop.send(());
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [16, 20, 22, 24, 32, 48, 64]
            .into_iter()
            .map(recording_icon)
            .collect()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.title(),
            description: format!("Saving to {}", self.output_path.display()),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::menu::StandardItem {
                label: "Stop recording".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.stop.send(());
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn recording_icon(size: i32) -> ksni::Icon {
    // Catppuccin Mocha Text, Base and Red, in network-byte-order ARGB.
    const TEXT: [u8; 4] = [255, 205, 214, 244];
    const BASE: [u8; 4] = [255, 30, 30, 46];
    const RED: [u8; 4] = [255, 243, 139, 168];

    // Snap all edges to whole pixels and offer native panel sizes so the host
    // needn't resize a jagged circle. The monitor and stop mark have square corners.
    let margin = (size / 8).max(2);
    let stroke = (size / 16).max(1);
    let top = margin;
    let bottom = size - margin - 3 * stroke;
    let mark_size = (size / 3).max(4);
    let mark_left = (size - mark_size) / 2;
    let mark_top = (top + bottom - mark_size) / 2;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let screen = (margin..size - margin).contains(&x) && (top..bottom).contains(&y);
            let interior = (margin + stroke..size - margin - stroke).contains(&x)
                && (top + stroke..bottom - stroke).contains(&y);
            let mark = (mark_left..mark_left + mark_size).contains(&x)
                && (mark_top..mark_top + mark_size).contains(&y);
            let stem = (size / 2 - stroke..size / 2 + stroke).contains(&x)
                && (bottom..bottom + stroke).contains(&y);
            let foot = (size / 4..size - size / 4).contains(&x)
                && (bottom + stroke..bottom + 2 * stroke).contains(&y);
            let pixel = if screen {
                if !interior {
                    TEXT
                } else if mark {
                    RED
                } else {
                    BASE
                }
            } else if stem || foot {
                TEXT
            } else {
                [0; 4]
            };
            data.extend_from_slice(&pixel);
        }
    }
    ksni::Icon {
        width: size,
        height: size,
        data,
    }
}

fn matches_recording(info: &RecordingPidInfo, pid: u32) -> bool {
    i32::try_from(pid).ok() == Some(info.pid)
}

pub(crate) fn run(pid: u32) -> Result<()> {
    let stop_requested = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create recording indicator runtime")?
        .block_on(monitor(pid))?;
    if stop_requested {
        // Use the same stop and post-recording menu flow as the sx assist.
        stop_recording_action()?;
    }
    Ok(())
}

async fn monitor(pid: u32) -> Result<bool> {
    let pid_file = get_pid_file();
    let info = match read_recording_pid_info(&pid_file) {
        Ok(info) if matches_recording(&info, pid) => info,
        _ => return Ok(false),
    };
    let output_path = info.output_path.clone().unwrap_or_default();
    let (stop, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let tray = RecordingTray {
        stop,
        output_path: output_path.clone(),
    };
    let registration = tokio::time::timeout(Duration::from_secs(5), tray.spawn())
        .await
        .context("Timed out registering recording tray icon")
        .and_then(|result| result.map_err(anyhow::Error::from));
    let handle = match registration {
        Ok(handle) => Some(handle),
        Err(err) => {
            eprintln!("Unable to show recording tray icon: {err}");
            let _ = show_notification(
                "Recording indicator unavailable",
                "Use the recording shortcut again to stop recording.",
            );
            None
        }
    };
    let stop_requested = loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        // An old helper must never stop a newer recording.
        if !read_recording_pid_info(&pid_file).is_ok_and(|current| {
            matches_recording(&current, pid) && current.output_path == info.output_path
        }) {
            break false;
        }
        let running = fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .map(|(_, tail)| !tail.starts_with('Z'))
            })
            .unwrap_or(false);
        if !running {
            cleanup_recording_state(&pid_file, &info.audio_module_ids);
            break false;
        }
        if requests.try_recv().is_ok() {
            break true;
        }
    };
    if let Some(handle) = handle {
        handle.shutdown().await;
    }
    Ok(stop_requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray;

    #[test]
    fn tray_controls_request_recording_stop() {
        let (stop, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let mut tray = RecordingTray {
            stop,
            output_path: PathBuf::from("recording.mp4"),
        };
        tray.activate(0, 0);
        assert!(requests.try_recv().is_ok());
        let mut menu = tray.menu();
        let ksni::MenuItem::Standard(item) = &mut menu[0] else {
            panic!("Expected stop recording menu item");
        };
        (item.activate)(&mut tray);
        assert!(requests.try_recv().is_ok());
    }

    #[test]
    fn helper_only_matches_its_own_recorder() {
        let info = RecordingPidInfo {
            pid: 42,
            output_path: None,
            audio_module_ids: vec![],
        };
        assert!(matches_recording(&info, 42));
        assert!(!matches_recording(&info, 43));
        assert!(!matches_recording(&info, u32::MAX));
    }
}
