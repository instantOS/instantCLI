//! instantWM display provider
//!
//! Uses instantwmctl IPC to query and configure display outputs on either
//! instantWM backend.

use super::{DisplayMode, OutputInfo};
use crate::common::instantwmctl;
use anyhow::{Context, Result};
use instantos_ipc::{DisplayModes, MonitorInfo};

/// instantWM display provider using instantwmctl.
pub struct InstantWMDisplayProvider;

impl InstantWMDisplayProvider {
    pub fn get_outputs_sync() -> Result<Vec<OutputInfo>> {
        let monitors: Vec<MonitorInfo> = instantwmctl::json(["monitor", "list"])?;
        let displays: Vec<DisplayModes> = instantwmctl::json(["monitor", "modes", "all"])?;
        let mut outputs = Vec::new();
        for display in &displays {
            if let Some(monitor) = monitors.iter().find(|monitor| monitor.name == display.name)
                && let Some(output) = output_info(display, monitor)?
            {
                outputs.push(output);
            }
        }

        Ok(outputs)
    }

    pub fn set_output_mode_sync(output_name: &str, mode: &DisplayMode) -> Result<()> {
        let resolution = format!("{}x{}", mode.width, mode.height);
        let rate = mode.refresh_label();

        instantwmctl::run([
            "monitor",
            "set",
            output_name,
            "--resolution",
            resolution.as_str(),
            "--refresh-rate",
            rate.as_str(),
        ])
        .with_context(|| format!("Failed to set mode for {} via instantwmctl", output_name))
    }
}

/// Turn one output's mode list into an [`OutputInfo`], or `None` when the
/// output has no modes to offer (an inactive head reports an empty list).
fn output_info(display: &DisplayModes, monitor: &MonitorInfo) -> Result<Option<OutputInfo>> {
    let mut modes: Vec<DisplayMode> = display
        .modes
        .iter()
        .map(|mode| {
            anyhow::ensure!(
                mode.width > 0 && mode.height > 0 && mode.refresh_millihertz >= 0,
                "Invalid display mode for {}: {mode:?}",
                display.name
            );
            Ok(DisplayMode {
                width: u32::try_from(mode.width)?,
                height: u32::try_from(mode.height)?,
                refresh: u32::try_from(mode.refresh_millihertz)?,
            })
        })
        .collect::<Result<_>>()?;

    // Sort by resolution (descending), then refresh rate (descending)
    modes.sort_by(|a, b| {
        b.resolution()
            .cmp(&a.resolution())
            .then(b.refresh.cmp(&a.refresh))
    });
    modes.dedup();

    if modes.is_empty() {
        return Ok(None);
    }

    // The mode the compositor reports for this output, falling back to the
    // first (highest) one. `monitor list` reports size but not refresh rate,
    // so the highest rate at the active size is the closest match available.
    let current_mode = modes
        .iter()
        .find(|mode| {
            i64::from(mode.width) == i64::from(monitor.width)
                && i64::from(mode.height) == i64::from(monitor.height)
        })
        .cloned()
        .unwrap_or_else(|| modes.first().cloned().unwrap());

    Ok(Some(OutputInfo {
        name: display.name.clone(),
        make: "Unknown".to_string(),
        model: "Unknown".to_string(),
        current_mode,
        available_modes: modes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, width: i32, height: i32) -> MonitorInfo {
        MonitorInfo {
            id: 1,
            position: 0,
            backend_index: 0,
            name: name.to_string(),
            width,
            height,
            x: 0,
            y: 0,
            is_selected: true,
            vrr_support: instantos_ipc::VrrSupport::Unsupported,
            vrr_mode: None,
            vrr_enabled: false,
            mirrors: Vec::new(),
            requested_mirrors: Vec::new(),
        }
    }

    fn modes(payload: &str) -> Vec<DisplayModes> {
        serde_json::from_str(payload).expect("monitor modes payload")
    }

    #[test]
    fn modes_are_sorted_by_resolution_then_refresh() {
        let output = output_info(
            &modes(
                r#"[{"name":"DP-1","modes":[
                    {"width":1920,"height":1080,"refresh_millihertz":60000},
                    {"width":1280,"height":1024,"refresh_millihertz":75025},
                    {"width":1920,"height":1080,"refresh_millihertz":164834}
                ]}]"#,
            )[0],
            &monitor("DP-1", 1920, 1080),
        )
        .expect("valid modes")
        .expect("output");

        let listed: Vec<(u32, u32, u32)> = output
            .available_modes
            .iter()
            .map(|mode| (mode.width, mode.height, mode.refresh))
            .collect();
        assert_eq!(
            listed,
            vec![
                (1920, 1080, 164834),
                (1920, 1080, 60000),
                (1280, 1024, 75025),
            ]
        );
        assert_eq!(output.current_mode.refresh, 164834);
        assert_eq!(output.name, "DP-1");
    }

    #[test]
    fn invalid_modes_fail_with_output_context() {
        let display = modes(
            r#"[{"name":"DP-1","modes":[{"width":-1920,"height":1080,"refresh_millihertz":60000}]}]"#,
        );
        let error = output_info(&display[0], &monitor("DP-1", 1920, 1080)).unwrap_err();
        assert!(error.to_string().contains("Invalid display mode for DP-1"));
    }

    #[test]
    fn duplicate_modes_are_collapsed() {
        let output = output_info(
            &modes(
                r#"[{"name":"DP-1","modes":[
                    {"width":1920,"height":1080,"refresh_millihertz":60000},
                    {"width":1920,"height":1080,"refresh_millihertz":60000}
                ]}]"#,
            )[0],
            &monitor("DP-1", 1920, 1080),
        )
        .expect("valid modes")
        .expect("output");

        assert_eq!(output.available_modes.len(), 1);
    }

    #[test]
    fn outputs_without_modes_are_skipped() {
        assert!(
            output_info(
                &modes(r#"[{"name":"eDP-1","modes":[]}]"#)[0],
                &monitor("eDP-1", 0, 0)
            )
            .expect("valid empty modes")
            .is_none()
        );
    }

    #[test]
    fn current_mode_falls_back_to_the_best_mode() {
        // A scaled output reports a logical size no mode matches exactly.
        let output = output_info(
            &modes(
                r#"[{"name":"DP-1","modes":[
                    {"width":3840,"height":2160,"refresh_millihertz":60000},
                    {"width":1920,"height":1080,"refresh_millihertz":60000}
                ]}]"#,
            )[0],
            &monitor("DP-1", 1280, 720),
        )
        .expect("valid modes")
        .expect("output");

        assert_eq!(output.current_mode.width, 3840);
        assert_eq!(output.current_mode.height, 2160);
    }
}
