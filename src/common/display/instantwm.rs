//! instantWM Wayland display provider
//!
//! Uses instantwmctl IPC to query and configure display outputs
//! when running instantWM on the Wayland backend.

use super::{DisplayMode, OutputInfo};
use anyhow::{Context, Result};
use std::process::Command;

/// instantWM Wayland display provider using instantwmctl.
pub struct InstantWMDisplayProvider;

impl InstantWMDisplayProvider {
    /// Get all connected outputs with their modes via instantwmctl
    pub fn get_outputs_sync() -> Result<Vec<OutputInfo>> {
        // Get available modes
        let modes_output = Command::new("instantwmctl")
            .args(["monitor", "modes", "--json"])
            .output()
            .context("Failed to execute instantwmctl monitor modes")?;

        if !modes_output.status.success() {
            let stderr = String::from_utf8_lossy(&modes_output.stderr);
            anyhow::bail!("instantwmctl monitor modes failed: {}", stderr);
        }

        let modes_stdout = String::from_utf8_lossy(&modes_output.stdout);
        let display_modes: Vec<serde_json::Value> = serde_json::from_str(&modes_stdout)
            .context("Failed to parse instantwmctl monitor modes JSON")?;

        // Get current monitor state
        let list_output = Command::new("instantwmctl")
            .args(["monitor", "list", "--json"])
            .output()
            .context("Failed to execute instantwmctl monitor list")?;

        let monitors: Vec<serde_json::Value> = if list_output.status.success() {
            let list_stdout = String::from_utf8_lossy(&list_output.stdout);
            serde_json::from_str(&list_stdout).unwrap_or_default()
        } else {
            Vec::new()
        };

        let mut outputs = Vec::new();

        for display in &display_modes {
            let name = display
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();

            let modes_json = display.get("modes").and_then(|v| v.as_array());

            let mut modes: Vec<DisplayMode> = modes_json
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| {
                            let width = m.get("width").and_then(|v| v.as_u64())? as u32;
                            let height = m.get("height").and_then(|v| v.as_u64())? as u32;
                            let refresh_mhz = m.get("refresh_mhz").and_then(|v| v.as_u64())? as u32;
                            Some(DisplayMode {
                                width,
                                height,
                                refresh: refresh_mhz,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            // Sort by resolution (descending), then refresh rate (descending)
            modes.sort_by(|a, b| {
                b.resolution()
                    .cmp(&a.resolution())
                    .then(b.refresh.cmp(&a.refresh))
            });
            modes.dedup();

            if modes.is_empty() {
                continue;
            }

            // Find current mode from monitor list (match by name, use width+height)
            let current_mode = monitors
                .iter()
                .find(|m| m.get("name").and_then(|v| v.as_str()) == Some(name.as_str()))
                .and_then(|m| {
                    let w = m.get("width").and_then(|v| v.as_u64())? as u32;
                    let h = m.get("height").and_then(|v| v.as_u64())? as u32;
                    modes
                        .iter()
                        .find(|mode| mode.width == w && mode.height == h)
                        .cloned()
                })
                .unwrap_or_else(|| modes.first().cloned().unwrap());

            outputs.push(OutputInfo {
                name,
                make: "Unknown".to_string(),
                model: "Unknown".to_string(),
                current_mode,
                available_modes: modes,
            });
        }

        Ok(outputs)
    }

    /// Set a display's mode via instantwmctl
    pub fn set_output_mode_sync(output_name: &str, mode: &DisplayMode) -> Result<()> {
        let resolution = format!("{}x{}", mode.width, mode.height);
        let rate = mode.refresh_hz() as f32;

        let status = Command::new("instantwmctl")
            .args([
                "monitor",
                "set",
                output_name,
                "--res",
                &resolution,
                "--rate",
                &rate.to_string(),
            ])
            .status()
            .context("Failed to execute instantwmctl")?;

        if !status.success() {
            anyhow::bail!(
                "Failed to set mode for {} via instantwmctl (exit code: {})",
                output_name,
                status.code().unwrap_or(-1)
            );
        }

        Ok(())
    }
}
