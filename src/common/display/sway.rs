//! Sway-specific display provider
//!
//! Uses swaymsg to query and configure display outputs.

use super::{DisplayMode, OutputInfo};
use anyhow::{Context, Result};
use tokio::process::Command as TokioCommand;

/// Sway display provider for querying and setting display modes
pub struct SwayDisplayProvider;

impl SwayDisplayProvider {
    /// Get all connected outputs with their modes
    pub async fn get_outputs() -> Result<Vec<OutputInfo>> {
        let output = TokioCommand::new("swaymsg")
            .args(["-t", "get_outputs"])
            .output()
            .await
            .context("Failed to execute swaymsg")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("swaymsg failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Self::parse_outputs(&stdout)
    }

    /// Get outputs synchronously (for use in settings apply)
    pub fn get_outputs_sync() -> Result<Vec<OutputInfo>> {
        let output = std::process::Command::new("swaymsg")
            .args(["-t", "get_outputs"])
            .output()
            .context("Failed to execute swaymsg")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("swaymsg failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Self::parse_outputs(&stdout)
    }

    /// Set a display's mode
    pub async fn set_output_mode(output_name: &str, mode: &DisplayMode) -> Result<()> {
        let mode_str = mode.to_swaymsg_format();
        let command = format!("output {} mode {}", output_name, mode_str);

        let output = TokioCommand::new("swaymsg")
            .arg(&command)
            .output()
            .await
            .context("Failed to execute swaymsg")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to set mode for {}: {}", output_name, stderr);
        }

        Ok(())
    }

    /// Set a display's mode synchronously
    pub fn set_output_mode_sync(output_name: &str, mode: &DisplayMode) -> Result<()> {
        let mode_str = mode.to_swaymsg_format();
        let command = format!("output {} mode {}", output_name, mode_str);

        let output = std::process::Command::new("swaymsg")
            .arg(&command)
            .output()
            .context("Failed to execute swaymsg")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to set mode for {}: {}", output_name, stderr);
        }

        Ok(())
    }

    /// Parse swaymsg -t get_outputs JSON
    fn parse_outputs(json_str: &str) -> Result<Vec<OutputInfo>> {
        let outputs: Vec<serde_json::Value> =
            serde_json::from_str(json_str).context("Failed to parse swaymsg output JSON")?;

        outputs
            .into_iter()
            .map(Self::parse_output_info)
            .collect::<Result<Vec<_>>>()
    }

    fn parse_output_info(output: serde_json::Value) -> Result<OutputInfo> {
        let name = output
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing output name"))?
            .to_string();

        let make = output
            .get("make")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let model = output
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let current_mode = Self::parse_current_mode(&output, &name)?;
        let available_modes = Self::parse_available_modes(&output, &name)?;

        Ok(OutputInfo {
            name,
            make,
            model,
            current_mode,
            available_modes,
        })
    }

    fn parse_current_mode(output: &serde_json::Value, name: &str) -> Result<DisplayMode> {
        let current_mode_json = output
            .get("current_mode")
            .ok_or_else(|| anyhow::anyhow!("Missing current_mode for {}", name))?;

        Self::parse_display_mode(current_mode_json).context("Invalid current_mode")
    }

    fn parse_available_modes(output: &serde_json::Value, name: &str) -> Result<Vec<DisplayMode>> {
        let modes_json = output
            .get("modes")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing modes array for {}", name))?;

        // Parse modes
        let mut modes: Vec<DisplayMode> = modes_json
            .iter()
            .filter_map(|mode| Self::parse_display_mode(mode).ok())
            .collect();

        // Sort by resolution (descending), then refresh (descending) to group duplicates together
        modes.sort_by(|a, b| {
            b.resolution()
                .cmp(&a.resolution())
                .then(b.refresh.cmp(&a.refresh))
        });

        // Remove duplicates (only removes consecutive duplicates, so sort is required first)
        modes.dedup();

        Ok(modes)
    }

    fn parse_display_mode(mode_json: &serde_json::Value) -> Result<DisplayMode> {
        Ok(DisplayMode {
            width: mode_json
                .get("width")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing width"))? as u32,
            height: mode_json
                .get("height")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing height"))? as u32,
            refresh: mode_json
                .get("refresh")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing refresh"))? as u32,
        })
    }
}
