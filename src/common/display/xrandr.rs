//! X11 display provider using xrandr
//!
//! Generic display provider for X11 desktops. Works with any window manager
//! (instantWM, i3, dwm, etc.) since xrandr is the standard X11 display tool.

use super::{DisplayMode, OutputInfo};
use anyhow::{Context, Result};
use std::process::Command;

/// X11 display provider using xrandr for mode queries and changes.
pub struct XrandrDisplayProvider;

impl XrandrDisplayProvider {
    /// Get all connected outputs with their modes via xrandr --json
    pub fn get_outputs_sync() -> Result<Vec<OutputInfo>> {
        let output = Command::new("xrandr")
            .arg("--json")
            .output()
            .context("Failed to execute xrandr (is xrandr installed?)")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("xrandr failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_xrandr_json(&stdout)
    }

    /// Set a display's mode via xrandr
    pub fn set_output_mode_sync(output_name: &str, mode: &DisplayMode) -> Result<()> {
        let mode_str = format!("{}x{}@{:.3}Hz", mode.width, mode.height, mode.refresh_hz());

        let status = Command::new("xrandr")
            .args(["--output", output_name, "--mode", &mode_str])
            .status()
            .context("Failed to execute xrandr")?;

        if !status.success() {
            anyhow::bail!(
                "Failed to set mode {} for {} via xrandr (exit code: {})",
                mode_str,
                output_name,
                status.code().unwrap_or(-1)
            );
        }

        Ok(())
    }
}

/// Parse xrandr --json output into OutputInfo list
fn parse_xrandr_json(json_str: &str) -> Result<Vec<OutputInfo>> {
    let json: serde_json::Value =
        serde_json::from_str(json_str).context("Failed to parse xrandr JSON output")?;

    let screens = json
        .get("screens")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Missing screens array in xrandr output"))?;

    let mut outputs = Vec::new();

    for screen in screens {
        let Some(outputs_json) = screen.get("outputs").and_then(|v| v.as_array()) else {
            continue;
        };

        for output_json in outputs_json {
            if let Some(info) = parse_xrandr_output(output_json) {
                outputs.push(info);
            }
        }
    }

    Ok(outputs)
}

/// Parse a single xrandr output entry
fn parse_xrandr_output(output: &serde_json::Value) -> Option<OutputInfo> {
    let name = output.get("name").and_then(|v| v.as_str())?.to_string();

    // Skip disconnected outputs
    let connection = output.get("connection").and_then(|v| v.as_str());
    if connection == Some("disconnected") {
        return None;
    }

    let make = output
        .get("manufacturer")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let model = output
        .get("product")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let modes_json = output.get("modes").and_then(|v| v.as_array())?;

    let mut modes: Vec<DisplayMode> = Vec::new();
    let mut current_mode: Option<DisplayMode> = None;

    for mode_json in modes_json {
        let width = mode_json.get("width").and_then(|v| v.as_u64())? as u32;
        let height = mode_json.get("height").and_then(|v| v.as_u64())? as u32;

        let Some(frequencies) = mode_json.get("frequencies").and_then(|v| v.as_array()) else {
            continue;
        };

        for freq in frequencies {
            let rate = freq.get("rate").and_then(|v| v.as_f64())?;
            let refresh = (rate * 1000.0) as u32;
            let is_current = freq
                .get("current")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mode = DisplayMode {
                width,
                height,
                refresh,
            };

            if is_current {
                current_mode = Some(mode.clone());
            }

            modes.push(mode);
        }
    }

    // Sort by resolution (descending), then refresh rate (descending)
    modes.sort_by(|a, b| {
        b.resolution()
            .cmp(&a.resolution())
            .then(b.refresh.cmp(&a.refresh))
    });
    modes.dedup();

    let current_mode = current_mode.unwrap_or_else(|| {
        modes.first().cloned().unwrap_or(DisplayMode {
            width: 0,
            height: 0,
            refresh: 0,
        })
    });

    // Skip outputs with no modes
    if modes.is_empty() {
        return None;
    }

    Some(OutputInfo {
        name,
        make,
        model,
        current_mode,
        available_modes: modes,
    })
}
