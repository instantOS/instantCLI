//! instantWM display provider
//!
//! Uses instantwmctl IPC to query and configure display outputs on either
//! instantWM backend.

use super::{DisplayMode, OutputInfo};
use crate::common::instantwmctl;
use anyhow::{Context, Result};
use serde::Deserialize;

/// One entry of `instantwmctl --json monitor list`.
#[derive(Debug, Clone, Deserialize)]
struct MonitorInfo {
    name: String,
    width: u32,
    height: u32,
}

/// One entry of `instantwmctl --json monitor modes <output>`.
#[derive(Debug, Clone, Deserialize)]
struct DisplayModes {
    name: String,
    modes: Vec<MonitorMode>,
}

#[derive(Debug, Clone, Deserialize)]
struct MonitorMode {
    width: u32,
    height: u32,
    refresh_mhz: u32,
}

/// instantWM display provider using instantwmctl.
pub struct InstantWMDisplayProvider;

impl InstantWMDisplayProvider {
    /// Get all connected outputs with their modes via instantwmctl
    pub fn get_outputs_sync() -> Result<Vec<OutputInfo>> {
        // Ask for the outputs first: `monitor modes` answers for a single
        // output (the focused one unless told otherwise), so every output is
        // queried in turn. (`--json` is a top-level instantwmctl flag and has
        // to precede the subcommand.)
        let monitors: Vec<MonitorInfo> = instantwmctl::json(["monitor", "list"])
            .context("Failed to execute instantwmctl monitor list")?;

        let mut outputs = Vec::new();

        for monitor in &monitors {
            let display_modes: Vec<DisplayModes> =
                instantwmctl::json(["monitor", "modes", &monitor.name]).with_context(|| {
                    format!(
                        "Failed to execute instantwmctl monitor modes for {}",
                        monitor.name
                    )
                })?;

            outputs.extend(
                display_modes
                    .iter()
                    .filter_map(|display| output_info(display, monitor)),
            );
        }

        Ok(outputs)
    }

    /// Set a display's mode via instantwmctl
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
fn output_info(display: &DisplayModes, monitor: &MonitorInfo) -> Option<OutputInfo> {
    let mut modes: Vec<DisplayMode> = display
        .modes
        .iter()
        .map(|mode| DisplayMode {
            width: mode.width,
            height: mode.height,
            refresh: mode.refresh_mhz,
        })
        .collect();

    // Sort by resolution (descending), then refresh rate (descending)
    modes.sort_by(|a, b| {
        b.resolution()
            .cmp(&a.resolution())
            .then(b.refresh.cmp(&a.refresh))
    });
    modes.dedup();

    if modes.is_empty() {
        return None;
    }

    // The mode the compositor reports for this output, falling back to the
    // first (highest) one. `monitor list` reports size but not refresh rate,
    // so the highest rate at the active size is the closest match available.
    let current_mode = modes
        .iter()
        .find(|mode| mode.width == monitor.width && mode.height == monitor.height)
        .cloned()
        .unwrap_or_else(|| modes.first().cloned().unwrap());

    Some(OutputInfo {
        name: display.name.clone(),
        make: "Unknown".to_string(),
        model: "Unknown".to_string(),
        current_mode,
        available_modes: modes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, width: u32, height: u32) -> MonitorInfo {
        MonitorInfo {
            name: name.to_string(),
            width,
            height,
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
                    {"width":1920,"height":1080,"refresh_mhz":60000},
                    {"width":1280,"height":1024,"refresh_mhz":75025},
                    {"width":1920,"height":1080,"refresh_mhz":164834}
                ]}]"#,
            )[0],
            &monitor("DP-1", 1920, 1080),
        )
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
    fn duplicate_modes_are_collapsed() {
        let output = output_info(
            &modes(
                r#"[{"name":"DP-1","modes":[
                    {"width":1920,"height":1080,"refresh_mhz":60000},
                    {"width":1920,"height":1080,"refresh_mhz":60000}
                ]}]"#,
            )[0],
            &monitor("DP-1", 1920, 1080),
        )
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
            .is_none()
        );
    }

    #[test]
    fn current_mode_falls_back_to_the_best_mode() {
        // A scaled output reports a logical size no mode matches exactly.
        let output = output_info(
            &modes(
                r#"[{"name":"DP-1","modes":[
                    {"width":3840,"height":2160,"refresh_mhz":60000},
                    {"width":1920,"height":1080,"refresh_mhz":60000}
                ]}]"#,
            )[0],
            &monitor("DP-1", 1280, 720),
        )
        .expect("output");

        assert_eq!(output.current_mode.width, 3840);
        assert_eq!(output.current_mode.height, 2160);
    }
}
