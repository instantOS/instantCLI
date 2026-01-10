use async_trait::async_trait;
use std::str::FromStr;
use strum_macros::{EnumString, IntoStaticStr};
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

/// Abstract performance mode
#[derive(Default)]
pub enum GeneralPowerMode {
    /// Absolute performance
    Performance,

    /// Power
    Balanced,

    /// Maximum power saving
    PowerSave,

    #[default]
    Unknown,
}

/// Adapts to one of the two possible ways described in the top comment
#[async_trait]
pub trait PowerHandle<T>
where
    T: From<GeneralPowerMode> + FromStr + Into<&'static str>,
{
    /// Retrieves the current performance mode
    async fn query_performance_mode(&self) -> T;

    /// Changes the performance mode, might require sudo
    /// Returns true on success
    async fn change_performance_mode(&mut self, mode: T) -> bool;
}

/// Performance modes used by `powerprofilesctl`
#[derive(Default, EnumString, IntoStaticStr)]
pub enum GnomePowerMode {
    #[strum(serialize = "performance")]
    Performance,

    #[strum(serialize = "balanced")]
    Balanced,

    #[strum(serialize = "power-saver")]
    PowerSaver,

    #[default]
    Unknown,
}

impl From<GeneralPowerMode> for GnomePowerMode {
    fn from(mode: GeneralPowerMode) -> Self {
        match mode {
            GeneralPowerMode::Performance => GnomePowerMode::Performance,
            GeneralPowerMode::Balanced => GnomePowerMode::Balanced,
            GeneralPowerMode::PowerSave => GnomePowerMode::PowerSaver,
            _ => GnomePowerMode::Unknown,
        }
    }
}

/// Implementation for `powerprofilesctl`
const PP_CTL: &str = "powerprofilesctl";
#[derive(Default)]
pub struct GnomePowerHandle;

#[async_trait]
impl PowerHandle<GnomePowerMode> for GnomePowerHandle {
    async fn query_performance_mode(&self) -> GnomePowerMode {
        let output = TokioCommand::new(PP_CTL).output();
        todo!();
    }

    async fn change_performance_mode(&mut self, mode: GnomePowerMode) -> bool {
        TokioCommand::new(PP_CTL)
            .args(["set", mode.into()])
            .output()
            .await
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

pub enum LegacyPowerMode {
    Performance,
    PowerSave,
    Unknown,
}

/// Uses `/sys/devices/system/cpu/cpu_/cpufreq` to control performance mode
#[derive(Default)]
pub struct LegacyPowerHandle;

impl From<&'static str> for LegacyPowerMode {
    /// Turns the value read from /sys/devices/system/cpu/cpu_/cpufreq/scaling_available_governors/
    /// into a variant of the PerformanceMode enum
    fn from(value: &'static str) -> Self {
        match value {
            "performance" => LegacyPowerMode::Performance,
            "powersave" => LegacyPowerMode::PowerSave,
            _ => LegacyPowerMode::Unknown,
        }
    }
}

/// This enum holds all supported power handles
pub enum PowerHandles {
    ProfiledPowerHandle(GnomePowerHandle),
    LegacyPowerHandle(LegacyPowerHandle),
}

/// Creates a power handle
#[derive(Default)]
pub struct PowerHandleFactory;

impl PowerHandleFactory {
    async fn build_power_handle(&self) -> Option<PowerHandles> {
        // We check if powerprofilesctl is available
        let profiled_power = TokioCommand::new("which")
            .arg("powerprofilesctl")
            .output()
            .await
            .map(|output| output.status.success())
            .unwrap_or(false);
        if profiled_power {
            return PowerHandles::ProfiledPowerHandle(GnomePowerHandle::default()).into();
        }

        // If not, we check if we have access to the sysfiles
        let sys_available =
            TokioFile::open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors")
                .await
                .is_ok();
        if sys_available {
            PowerHandles::LegacyPowerHandle(LegacyPowerHandle::default()).into()
        } else {
            // Unfortunately we have no way to control power management
            None
        }
    }
}
