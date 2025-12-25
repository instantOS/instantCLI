//! Sway display check - ensures displays are running at optimal settings

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use crate::common::display::{OutputInfo, SwayDisplayProvider};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Default)]
pub struct SwayDisplayCheck;

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

        match SwayDisplayProvider::get_outputs().await {
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
                        .map(|o| format!("{}: {}", o.name, o.current_mode.display_format()))
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
                                "{}: {} (optimal: {})",
                                o.name,
                                o.current_mode.display_format(),
                                o.optimal_mode().display_format()
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
        let outputs = SwayDisplayProvider::get_outputs().await?;

        let mut fixed = 0;
        for output in outputs {
            if !output.is_optimal() {
                let optimal = output.optimal_mode();
                println!(
                    "Setting {} to {}...",
                    output.name,
                    optimal.display_format()
                );
                SwayDisplayProvider::set_output_mode(&output.name, &optimal).await?;
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
