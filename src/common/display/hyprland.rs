//! Hyprland display provider using hyprctl
//!
//! Queries monitor state via `hyprctl monitors all -j` and applies mode
//! changes by rewriting the full monitor rule so position/scale stay intact.

use super::{DisplayMode, OutputInfo};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

/// Hyprland display provider using hyprctl.
pub struct HyprlandDisplayProvider;

impl HyprlandDisplayProvider {
    /// Get all active outputs with their modes via hyprctl.
    pub fn get_outputs_sync() -> Result<Vec<OutputInfo>> {
        let monitors = get_monitors()?;

        let mut outputs = Vec::new();

        for monitor in monitors.into_iter().filter(|monitor| !monitor.disabled) {
            let available_modes = parse_available_modes(&monitor.available_modes);
            if available_modes.is_empty() {
                continue;
            }

            let current_mode = current_mode(&monitor).unwrap_or_else(|| {
                available_modes.first().cloned().unwrap_or(DisplayMode {
                    width: 0,
                    height: 0,
                    refresh: 0,
                })
            });

            outputs.push(OutputInfo {
                name: monitor.name,
                make: monitor.make,
                model: monitor.model,
                current_mode,
                available_modes,
            });
        }

        Ok(outputs)
    }

    /// Set a display's mode via hyprctl while preserving its placement and extras.
    pub fn set_output_mode_sync(output_name: &str, mode: &DisplayMode) -> Result<()> {
        let monitor = get_monitors()?
            .into_iter()
            .find(|monitor| monitor.name == output_name)
            .ok_or_else(|| anyhow::anyhow!("Monitor '{}' not found", output_name))?;

        let mut rule = format!(
            "{},{},{},{}",
            monitor.name,
            mode.to_hyprland_format(),
            monitor.position_string(),
            monitor.scale_string()
        );

        if monitor.transform != 0 {
            rule.push_str(&format!(",transform,{}", monitor.transform));
        }

        if let Some(mirror_target) = monitor.mirror_target() {
            rule.push_str(&format!(",mirror,{}", mirror_target));
        }

        if let Some(vrr_mode) = monitor.vrr_mode() {
            rule.push_str(&format!(",vrr,{}", vrr_mode));
        }

        if let Some(color_preset) = monitor.color_management_preset.as_deref()
            && color_preset != "srgb"
        {
            rule.push_str(&format!(",cm,{}", color_preset));
        }

        if let Some(value) = monitor.sdr_brightness
            && (value - 1.0).abs() > f64::EPSILON
        {
            rule.push_str(&format!(",sdrbrightness,{}", trim_float(value)));
        }

        if let Some(value) = monitor.sdr_saturation
            && (value - 1.0).abs() > f64::EPSILON
        {
            rule.push_str(&format!(",sdrsaturation,{}", trim_float(value)));
        }

        let output = Command::new("hyprctl")
            .args(["keyword", "monitor", &rule])
            .output()
            .context("Failed to execute hyprctl keyword monitor")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to set mode for {}: {}", output_name, stderr.trim());
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct HyprlandMonitor {
    name: String,
    make: String,
    model: String,
    width: i64,
    height: i64,
    #[serde(rename = "refreshRate")]
    refresh_rate: f64,
    x: i64,
    y: i64,
    scale: f64,
    transform: i64,
    disabled: bool,
    #[serde(rename = "mirrorOf")]
    mirror_of: Option<String>,
    vrr: Option<bool>,
    #[serde(rename = "availableModes", default)]
    available_modes: Vec<String>,
    #[serde(rename = "colorManagementPreset")]
    color_management_preset: Option<String>,
    #[serde(rename = "sdrBrightness")]
    sdr_brightness: Option<f64>,
    #[serde(rename = "sdrSaturation")]
    sdr_saturation: Option<f64>,
}

impl HyprlandMonitor {
    fn position_string(&self) -> String {
        format!("{}x{}", self.x, self.y)
    }

    fn scale_string(&self) -> String {
        trim_float(self.scale)
    }

    fn mirror_target(&self) -> Option<&str> {
        self.mirror_of
            .as_deref()
            .filter(|value| !value.is_empty() && *value != "none")
    }

    fn vrr_mode(&self) -> Option<u8> {
        self.vrr.map(u8::from)
    }
}

fn get_monitors() -> Result<Vec<HyprlandMonitor>> {
    let output = Command::new("hyprctl")
        .args(["-j", "monitors", "all"])
        .output()
        .context("Failed to execute hyprctl monitors")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("hyprctl monitors failed: {}", stderr.trim());
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse hyprctl monitors JSON output")
}

fn current_mode(monitor: &HyprlandMonitor) -> Option<DisplayMode> {
    if monitor.width <= 0 || monitor.height <= 0 || monitor.refresh_rate <= 0.0 {
        return None;
    }

    Some(DisplayMode {
        width: monitor.width as u32,
        height: monitor.height as u32,
        refresh: (monitor.refresh_rate * 1000.0).round() as u32,
    })
}

fn parse_available_modes(modes: &[String]) -> Vec<DisplayMode> {
    let mut parsed: Vec<DisplayMode> = modes
        .iter()
        .filter_map(|mode| parse_mode_string(mode).ok())
        .collect();

    parsed.sort_by(|a, b| {
        b.resolution()
            .cmp(&a.resolution())
            .then(b.refresh.cmp(&a.refresh))
    });
    parsed.dedup();
    parsed
}

fn parse_mode_string(mode: &str) -> Result<DisplayMode> {
    let (resolution, refresh_part) = mode
        .split_once('@')
        .ok_or_else(|| anyhow::anyhow!("Missing refresh separator"))?;
    let (width, height) = resolution
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("Missing resolution separator"))?;

    let refresh = refresh_part.trim_end_matches("Hz").parse::<f64>()?;

    Ok(DisplayMode {
        width: width.parse()?,
        height: height.parse()?,
        refresh: (refresh * 1000.0).round() as u32,
    })
}

fn trim_float(value: f64) -> String {
    let mut trimmed = format!("{value:.3}");
    while trimmed.contains('.') && trimmed.ends_with('0') {
        trimmed.pop();
    }
    if trimmed.ends_with('.') {
        trimmed.pop();
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::{parse_mode_string, trim_float};

    #[test]
    fn parses_hyprland_mode_string() {
        let mode = parse_mode_string("1920x1080@59.94Hz").unwrap();
        assert_eq!(mode.width, 1920);
        assert_eq!(mode.height, 1080);
        assert_eq!(mode.refresh, 59_940);
    }

    #[test]
    fn trims_float_for_monitor_rules() {
        assert_eq!(trim_float(1.0), "1");
        assert_eq!(trim_float(1.25), "1.25");
        assert_eq!(trim_float(59.94), "59.94");
    }
}
