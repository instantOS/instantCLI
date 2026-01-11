use crate::common::deps;
use crate::doctor::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, anyhow};
use async_trait::async_trait;
use std::str::FromStr;
use strum_macros::{Display, EnumString};
use tokio::fs::File as TokioFile;
use tokio::process::Command as TokioCommand;

///
/// Performance <-> Power-Saving
///
/// Many laptops automatically activate power saving mode and sometimes do not
/// switch back to performance, even when plugged in.
/// This is a UNIX-only feature
/// We try to query the current power authority and get the mode
/// First we try "powerprofilesctl", if available, we manage power using this tool
/// Otherwise, we manually insert our preference:
/// The directory /sys/devices/system/cpu/cpu*/cpufreq/ provides information
/// about the current CPU power mode, * is to replace with the id of the core.
///

/// Adapts to one of the two possible ways described in the top comment
#[async_trait]
pub trait PowerHandle: Send + Sync {
    /// Retrieves the current performance mode
    async fn query_performance_mode(&self) -> anyhow::Result<PowerMode>;

    /// Changes the performance mode, might require sudo
    /// Returns true on success
    async fn change_performance_mode(&self, mode: PowerMode) -> anyhow::Result<()>;

    /// Returns all available performance modes on the system
    async fn available_modes(&self) -> Vec<PowerMode>;
}

/// Performance modes
#[derive(PartialEq, EnumString, Debug, Display)]
pub enum PowerMode {
    #[strum(serialize = "performance")]
    Performance,

    #[strum(serialize = "balanced")]
    Balanced,

    #[strum(serialize = "power-saver", serialize = "powersave")]
    PowerSaver,
}

/// Implementation for `powerprofilesctl`
const PP_CTL: &str = "powerprofilesctl";
#[derive(Default)]
pub struct GnomePowerHandle;

#[async_trait]
impl PowerHandle for GnomePowerHandle {
    async fn query_performance_mode(&self) -> anyhow::Result<PowerMode> {
        let output = TokioCommand::new(PP_CTL).arg("get").output().await?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(PowerMode::from_str(stdout.as_ref().trim())?)
        } else {
            Err(anyhow!("{} get returned non-zero value", PP_CTL))
        }
    }

    async fn change_performance_mode(&self, mode: PowerMode) -> anyhow::Result<()> {
        let gnome_identifier = match mode {
            PowerMode::Performance => "performance",
            PowerMode::Balanced => "balanced",
            PowerMode::PowerSaver => "power-save",
        };

        let success = TokioCommand::new(PP_CTL)
            .args(["set", gnome_identifier])
            .output()
            .await
            .map(|output| output.status.success())?;
        if !success {
            Err(anyhow!("Failed to set power-saver mode"))
        } else {
            Ok(())
        }
    }

    async fn available_modes(&self) -> Vec<PowerMode> {
        let output = TokioCommand::new(PP_CTL).arg("list").output().await;

        match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout
                    .lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        if line.ends_with(':') {
                            let profile_name =
                                line.trim_end_matches(':').trim_start_matches('*').trim();
                            profile_name.parse().ok()
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            _ => vec![],
        }
    }
}

/// Uses `/sys/devices/system/cpu/cpu_/cpufreq` to control performance mode
#[derive(Default)]
pub struct LegacyPowerHandle;

#[async_trait]
impl PowerHandle for LegacyPowerHandle {
    async fn query_performance_mode(&self) -> anyhow::Result<PowerMode> {
        const GOVERNOR_PATH: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";

        let content = tokio::fs::read_to_string(GOVERNOR_PATH).await?;
        Ok(PowerMode::from_str(content.trim())?)
    }

    async fn change_performance_mode(&self, mode: PowerMode) -> anyhow::Result<()> {
        let available_modes = self.available_modes().await;

        if !available_modes.contains(&mode) {
            return Err(anyhow!("Power mode {} is not available", mode));
        }

        let legacy_idenitfier = match mode {
            PowerMode::Performance | PowerMode::Balanced => "performance",
            PowerMode::PowerSaver => "powersave",
        };

        let mut cpu_dirs = match tokio::fs::read_dir("/sys/devices/system/cpu").await {
            Ok(dir) => dir,
            Err(_) => return Err(anyhow!("Failed to read dir /sys/devices/system/cpu")),
        };

        loop {
            let entry_opt = match cpu_dirs.next_entry().await {
                Ok(entry) => entry,
                Err(_) => {
                    return Err(anyhow!(
                        "Failed to read directory entry from /sys/devices/system/cpu"
                    ));
                }
            };

            if let Some(entry) = entry_opt {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                if !name_str.starts_with("cpu") {
                    continue;
                }

                if let Ok(core) = name_str[3..].parse::<u32>() {
                    let governor_path = format!(
                        "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor",
                        core
                    );

                    tokio::fs::write(&governor_path, legacy_idenitfier)
                        .await
                        .with_context(|| format!("writing to {}", governor_path))?;
                }
            } else {
                return Ok(());
            }
        }
    }

    async fn available_modes(&self) -> Vec<PowerMode> {
        const AVAILABLE_GOVERNORS_PATH: &str =
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors";

        match tokio::fs::read_to_string(AVAILABLE_GOVERNORS_PATH).await {
            Ok(content) => content
                .split_whitespace()
                .filter_map(|governor| governor.parse().ok())
                .collect(),
            Err(_) => vec![],
        }
    }
}

/// Creates a power handle
#[derive(Default)]
pub struct PowerHandleFactory;

impl PowerHandleFactory {
    async fn build_power_handle(&self) -> anyhow::Result<Box<dyn PowerHandle>> {
        // We check if powerprofilesctl is available
        let profiled_power = deps::POWERPROFILESDAEMON.is_installed();
        if profiled_power {
            return Ok(Box::new(GnomePowerHandle::default()));
        }

        // If not, we check if we have access to the sysfiles
        let sys_available =
            TokioFile::open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors")
                .await
                .is_ok();
        if sys_available {
            Ok(Box::new(LegacyPowerHandle::default()))
        } else {
            // Unfortunately we have no way to control power management
            Err(anyhow!("Power management is not available"))
        }
    }
}

#[derive(Default)]
pub struct PerformanceTest;

impl PerformanceTest {
    async fn try_execute(&self) -> anyhow::Result<CheckStatus> {
        let handle = PowerHandleFactory::default().build_power_handle().await?;
        let mode = handle.query_performance_mode().await?;
        if mode != PowerMode::Performance {
            Ok(CheckStatus::Warning {
                message: format!("Power mode is not performance but {}", mode),
                fixable: true,
            })
        } else {
            Ok(CheckStatus::Pass("Power mode is performance".into()))
        }
    }
}

#[async_trait]
impl DoctorCheck for PerformanceTest {
    fn name(&self) -> &'static str {
        "Performance Mode"
    }

    fn id(&self) -> &'static str {
        "performance"
    }

    async fn execute(&self) -> CheckStatus {
        if let Ok(status) = self.try_execute().await {
            status
        } else {
            CheckStatus::Skipped("Could not query performance mode".into())
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Set power mode to performance".into())
    }

    async fn fix(&self) -> anyhow::Result<()> {
        let handle = PowerHandleFactory::default().build_power_handle().await?;
        handle.change_performance_mode(PowerMode::Performance).await
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_factory() {
        let factory = PowerHandleFactory::default();
        let power_handle = factory.build_power_handle().await.unwrap();
        println!(
            "Current mode: {:?}",
            power_handle.query_performance_mode().await.unwrap()
        );
        println!("Power modes:");
        for power_mode in power_handle.available_modes().await {
            println!("{:?}", power_mode)
        }
    }
}
