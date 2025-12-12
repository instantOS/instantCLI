use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

/// Represents a display mode with resolution and refresh rate
#[derive(Debug, Clone, PartialEq)]
struct DisplayMode {
    width: u32,
    height: u32,
    refresh: u32, // in milliHz (e.g., 164834 = 164.834 Hz)
}

impl DisplayMode {
    /// Resolution as total pixels
    fn resolution(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Refresh rate in Hz for display
    fn refresh_hz(&self) -> f64 {
        self.refresh as f64 / 1000.0
    }

    /// Format for swaymsg command (e.g., "1920x1080@164.834Hz")
    fn to_swaymsg_format(&self) -> String {
        format!("{}x{}@{:.3}Hz", self.width, self.height, self.refresh_hz())
    }
}

/// Information about a display output
#[derive(Debug)]
struct OutputInfo {
    name: String,
    current_mode: DisplayMode,
    optimal_mode: DisplayMode,
}

impl OutputInfo {
    fn is_optimal(&self) -> bool {
        self.current_mode == self.optimal_mode
    }
}

#[derive(Default)]
pub struct SwayDisplayCheck;

impl SwayDisplayCheck {
    /// Parse swaymsg -t get_outputs JSON and extract output info
    fn parse_outputs(json_str: &str) -> Result<Vec<OutputInfo>> {
        let outputs: Vec<serde_json::Value> = 
            serde_json::from_str(json_str).context("Failed to parse swaymsg output JSON")?;

        let mut result = Vec::new();

        for output in outputs {
            // Get output name
            let name = output
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing output name"))?
                .to_string();

            // Get current mode
            let current_mode_json = output
                .get("current_mode")
                .ok_or_else(|| anyhow::anyhow!("Missing current_mode for {}", name))?;

            let current_mode = DisplayMode {
                width: current_mode_json
                    .get("width")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing width in current_mode"))?
                    as u32,
                height: current_mode_json
                    .get("height")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing height in current_mode"))?
                    as u32,
                refresh: current_mode_json
                    .get("refresh")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing refresh in current_mode"))?
                    as u32,
            };

            // Get all available modes and find optimal
            let modes_json = output
                .get("modes")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing modes array for {}", name))?;

            let mut modes: Vec<DisplayMode> = Vec::new();
            for mode in modes_json {
                if let (Some(w), Some(h), Some(r)) = (
                    mode.get("width").and_then(|v| v.as_u64()),
                    mode.get("height").and_then(|v| v.as_u64()),
                    mode.get("refresh").and_then(|v| v.as_u64()),
                ) {
                    modes.push(DisplayMode {
                        width: w as u32,
                        height: h as u32,
                        refresh: r as u32,
                    });
                }
            }

            // Find optimal mode: highest resolution, then highest refresh rate
            let optimal_mode = modes
                .iter()
                .max_by(|a, b| {
                    // First compare by resolution
                    a.resolution()
                        .cmp(&b.resolution())
                        // Then by refresh rate
                        .then(a.refresh.cmp(&b.refresh))
                })
                .cloned()
                .unwrap_or_else(|| current_mode.clone());

            result.push(OutputInfo {
                name,
                current_mode,
                optimal_mode,
            });
        }

        Ok(result)
    }

    /// Get outputs using swaymsg
    async fn get_outputs() -> Result<Vec<OutputInfo>> {
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

    /// Set output mode using swaymsg
    async fn set_output_mode(output_name: &str, mode: &DisplayMode) -> Result<()> {
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
}

#[async_trait]
impl DoctorCheck for SwayDisplayCheck {
    fn name(&self) -> &'static str {
        "Sway Display Configuration"
    }

    fn id(&self) -> &'static str {
        "sway-display"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User // swaymsg runs as user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User // swaymsg runs as user
    }

    async fn execute(&self) -> CheckStatus {
        use crate::common::compositor::CompositorType;

        // Only run on Sway
        if CompositorType::detect() != CompositorType::Sway {
            return CheckStatus::Skipped("Not running on Sway".to_string());
        }

        match Self::get_outputs().await {
            Ok(outputs) => {
                if outputs.is_empty() {
                    return CheckStatus::Pass("No displays detected".to_string());
                }

                let suboptimal: Vec<&OutputInfo> =
                    outputs.iter().filter(|o| !o.is_optimal()).collect();

                if suboptimal.is_empty() {
                    // All displays are optimal
                    let summary: Vec<String> = outputs
                        .iter()
                        .map(|o| {
                            format!(
                                "{}: {}x{}@{:.0}Hz",
                                o.name,
                                o.current_mode.width,
                                o.current_mode.height,
                                o.current_mode.refresh_hz()
                            )
                        })
                        .collect();
                    CheckStatus::Pass(format!(
                        "All displays at optimal settings ({})",
                        summary.join(", ")
                    ))
                } else {
                    // Some displays are not optimal
                    let issues: Vec<String> = suboptimal
                        .iter()
                        .map(|o| {
                            format!(
                                "{}: {}x{}@{:.0}Hz (optimal: {}x{}@{:.0}Hz)",
                                o.name,
                                o.current_mode.width,
                                o.current_mode.height,
                                o.current_mode.refresh_hz(),
                                o.optimal_mode.width,
                                o.optimal_mode.height,
                                o.optimal_mode.refresh_hz()
                            )
                        })
                        .collect();
                    CheckStatus::Warning {
                        message: format!(
                            "Display(s) not at optimal settings: {}",
                            issues.join("; ")
                        ),
                        fixable: true,
                    }
                }
            }
            Err(e) => CheckStatus::Fail {
                message: format!("Failed to query displays: {}", e),
                fixable: false,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Set all displays to their maximum resolution and refresh rate".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let outputs = Self::get_outputs().await?;

        let mut fixed = 0;
        for output in outputs {
            if !output.is_optimal() {
                println!(
                    "Setting {} to {}x{}@{:.0}Hz...",
                    output.name,
                    output.optimal_mode.width,
                    output.optimal_mode.height,
                    output.optimal_mode.refresh_hz()
                );
                Self::set_output_mode(&output.name, &output.optimal_mode).await?;
                fixed += 1;
            }
        }

        if fixed == 0 {
            println!("All displays already at optimal settings.");
        } else {
            println!("Fixed {} display(s).", fixed);
        }

        Ok(())
    }
}