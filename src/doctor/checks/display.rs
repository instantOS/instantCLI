//! Sway display check - ensures displays are running at optimal settings

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use crate::common::compositor::config::{WindowManager, WmConfigManager};
use crate::common::display::{OutputInfo, SwayDisplayProvider};
use anyhow::Result;
use async_trait::async_trait;
use tokio::fs;

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
                println!("Setting {} to {}...", output.name, optimal.display_format());
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

#[derive(Default)]
pub struct SwaySetupCheck;

#[async_trait]
impl DoctorCheck for SwaySetupCheck {
    fn name(&self) -> &'static str {
        "Sway Setup Configuration"
    }

    fn id(&self) -> &'static str {
        "sway-setup"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        use crate::common::compositor::CompositorType;

        if CompositorType::detect() != CompositorType::Sway {
            return CheckStatus::Skipped("Sway is not active".to_string());
        }

        let manager = WmConfigManager::new(WindowManager::Sway);
        let config_path = manager.config_path();
        let main_config_path = manager.main_config_path();

        if !config_path.exists() || !main_config_path.exists() {
            return CheckStatus::Skipped("Sway config not found".to_string());
        }

        let expected = match crate::setup::generate_sway_config() {
            Ok(content) => content,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Failed to generate expected Sway config: {}", e),
                    fixable: false,
                };
            }
        };

        let actual = match fs::read_to_string(config_path).await {
            Ok(content) => content,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Failed to read Sway config: {}", e),
                    fixable: false,
                };
            }
        };

        let include_present = match manager.is_included_in_main_config() {
            Ok(present) => present,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Failed to read Sway main config: {}", e),
                    fixable: false,
                };
            }
        };

        let mut issues = Vec::new();
        if actual != expected {
            issues.push("shared config differs from generated output".to_string());
        }
        if !include_present {
            issues.push("main config missing instantCLI include".to_string());
        }

        if issues.is_empty() {
            CheckStatus::Pass("Sway config is up to date".to_string())
        } else {
            CheckStatus::Warning {
                message: format!("Sway setup needs refresh: {}", issues.join("; ")),
                fixable: true,
            }
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some(format!(
            "Run `{}` setup sway to regenerate and reload the Sway config",
            env!("CARGO_BIN_NAME")
        ))
    }

    async fn fix(&self) -> Result<()> {
        use crate::setup::{SetupCommands, handle_setup_command};
        handle_setup_command(SetupCommands::Sway)?;
        Ok(())
    }
}
