use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use ksni::TrayMethods;

use super::{
    RecordingPidInfo, cleanup_recording_state, get_pid_file, read_recording_pid_info,
    show_notification, stop_recording,
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
        vec![recording_icon()]
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

fn recording_icon() -> ksni::Icon {
    let mut data = Vec::with_capacity(32 * 32 * 4);
    for y in 0_i32..32 {
        for x in 0_i32..32 {
            let inside = (x - 16).pow(2) + (y - 16).pow(2) <= 13 * 13;
            // StatusNotifier pixmaps use network-byte-order ARGB.
            data.extend_from_slice(if inside { &[255, 235, 45, 45] } else { &[0; 4] });
        }
    }
    ksni::Icon {
        width: 32,
        height: 32,
        data,
    }
}

fn matches_recording(info: &RecordingPidInfo, pid: u32) -> bool {
    i32::try_from(pid).ok() == Some(info.pid)
}

pub(crate) fn run(pid: u32) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create recording indicator runtime")?
        .block_on(monitor(pid))
}

async fn monitor(pid: u32) -> Result<()> {
    let pid_file = get_pid_file();
    let info = match read_recording_pid_info(&pid_file) {
        Ok(info) if matches_recording(&info, pid) => info,
        _ => return Ok(()),
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
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        // An old helper must never stop a newer recording.
        if !read_recording_pid_info(&pid_file).is_ok_and(|current| {
            matches_recording(&current, pid) && current.output_path == info.output_path
        }) {
            break;
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
            break;
        }
        if requests.try_recv().is_ok() {
            stop_recording()?;
            show_notification(
                "Recording saved",
                &format!("Saved to {}", output_path.display()),
            )?;
            break;
        }
    }
    if let Some(handle) = handle {
        handle.shutdown().await;
    }
    Ok(())
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
