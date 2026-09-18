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
    /// Get all connected outputs with their modes via stock xrandr output.
    pub fn get_outputs_sync() -> Result<Vec<OutputInfo>> {
        let output = Command::new("xrandr")
            .arg("--query")
            .output()
            .context("Failed to execute xrandr (is xrandr installed?)")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("xrandr failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_xrandr_query(&stdout))
    }

    /// Set a display's mode via xrandr
    pub fn set_output_mode_sync(output_name: &str, mode: &DisplayMode) -> Result<()> {
        let mode_str = format!("{}x{}", mode.width, mode.height);
        let refresh = mode.refresh_label();

        let status = Command::new("xrandr")
            .args([
                "--output",
                output_name,
                "--mode",
                &mode_str,
                "--rate",
                &refresh,
            ])
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

/// Parse the stable, line-oriented output produced by `xrandr --query`.
fn parse_xrandr_query(query: &str) -> Vec<OutputInfo> {
    let mut outputs = Vec::new();
    let mut current: Option<OutputInfo> = None;

    for line in query.lines() {
        if !line.starts_with(char::is_whitespace) {
            if let Some(output) = current
                .take()
                .filter(|output| !output.available_modes.is_empty())
            {
                outputs.push(output);
            }

            let mut fields = line.split_whitespace();
            let Some(name) = fields.next() else { continue };
            if fields.next() != Some("connected") {
                continue;
            }
            current = Some(OutputInfo {
                name: name.to_string(),
                make: "Unknown".to_string(),
                model: "Unknown".to_string(),
                current_mode: DisplayMode {
                    width: 0,
                    height: 0,
                    refresh: 0,
                },
                available_modes: Vec::new(),
            });
            continue;
        }

        let Some(output) = current.as_mut() else {
            continue;
        };
        let mut fields = line.split_whitespace();
        let Some(resolution) = fields.next() else {
            continue;
        };
        let Some((width, height)) = resolution.split_once('x') else {
            continue;
        };
        let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>()) else {
            continue;
        };

        for rate_field in fields {
            let is_current = rate_field.contains('*');
            let rate = rate_field.trim_end_matches(['*', '+', 'i']);
            let Ok(rate) = rate.parse::<f64>() else {
                continue;
            };
            let mode = DisplayMode {
                width,
                height,
                refresh: (rate * 1000.0).round() as u32,
            };
            if is_current {
                output.current_mode = mode.clone();
            }
            output.available_modes.push(mode);
        }
    }

    if let Some(output) = current.filter(|output| !output.available_modes.is_empty()) {
        outputs.push(output);
    }

    for output in &mut outputs {
        output.available_modes.sort_by(|a, b| {
            b.resolution()
                .cmp(&a.resolution())
                .then(b.refresh.cmp(&a.refresh))
        });
        output.available_modes.dedup();
        if output.current_mode.width == 0 {
            output.current_mode = output.available_modes[0].clone();
        }
    }

    outputs
}

#[cfg(test)]
mod tests {
    use super::parse_xrandr_query;

    #[test]
    fn parses_stock_xrandr_query_output() {
        let outputs = parse_xrandr_query(concat!(
            "Screen 0: minimum 8 x 8, current 4480 x 1440, maximum 32767 x 32767\n",
            "DP-1 connected primary 2560x1440+0+0 (normal left inverted right x axis y axis)\n",
            "   2560x1440     59.95*+  120.00\n",
            "   1920x1080     60.00\n",
            "HDMI-1 disconnected (normal left inverted right x axis y axis)\n",
            "DP-2 connected 1920x1080+2560+0\n",
            "   1920x1080     60.00*+  59.94\n",
        ));

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].name, "DP-1");
        assert_eq!(outputs[0].current_mode.width, 2560);
        assert_eq!(outputs[0].current_mode.refresh, 59_950);
        assert_eq!(outputs[0].available_modes.len(), 3);
        assert_eq!(outputs[1].name, "DP-2");
        assert_eq!(outputs[1].current_mode.height, 1080);
    }
}
