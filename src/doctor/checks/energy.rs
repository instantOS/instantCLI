use crate::doctor::{CheckStatus, DoctorCheck};
use async_trait::async_trait;
use battery::Battery;

#[async_trait]
trait BatteryCheck: DoctorCheck {
    /// Called asynchronously with batteries containing at least one item
    fn check_parameter(&self, batteries: Vec<Battery>) -> CheckStatus;

    /// Ensures that at least one battery is present
    async fn execute(&self) -> CheckStatus {
        if let Ok(manager) = battery::Manager::new()
            && let Ok(maybe_batteries) = manager.batteries()
        {
            let batteries = maybe_batteries
                .filter(|maybe_battery| maybe_battery.is_ok())
                .map(|battery| battery.unwrap())
                .collect::<Vec<_>>();
            if batteries.is_empty() {
                return CheckStatus::Skipped("No batteries found".into());
            }

            self.check_parameter(batteries)
        } else {
            CheckStatus::Fail {
                message: "Could not initialize battery manager".to_string(),
                fixable: false,
            }
        }
    }

    fn format_battery(&self, battery: &Battery) -> String {
        format!(
            "{} ({})",
            battery.model().unwrap_or("Unknown model"),
            battery.serial_number().unwrap_or("Unknown S/N"),
        )
    }
}

#[derive(Default)]
pub struct PowerCheck;

#[async_trait]
impl DoctorCheck for PowerCheck {
    fn name(&self) -> &'static str {
        "Power level".into()
    }

    fn id(&self) -> &'static str {
        "power".into()
    }

    async fn execute(&self) -> CheckStatus {
        BatteryCheck::execute(self).await
    }
}

impl BatteryCheck for PowerCheck {
    fn check_parameter(&self, mut batteries: Vec<Battery>) -> CheckStatus {
        // Ordering by percentage
        batteries.sort_by(|b1, b2| {
            b1.state_of_charge()
                .value
                .total_cmp(&b2.state_of_charge().value)
        });

        // Get battery with the lowest charge
        let lowest = batteries.first().unwrap();
        let lowest_charge = lowest.state_of_charge();
        let battery_str = self.format_battery(lowest);
        let percent = (lowest_charge.value * 100.0) as u64;
        match lowest_charge.value {
            0.0..0.25 => CheckStatus::Fail {
                message: format!("{} - Critical power: {}%", battery_str, percent),
                fixable: false,
            },
            0.25..0.5 => CheckStatus::Warning {
                message: format!("{} - Low power: {}%", battery_str, percent),
                fixable: false,
            },
            _ => CheckStatus::Pass(format!("{} - Power OK: {}%", battery_str, percent)),
        }
    }
}

#[derive(Default)]
pub struct BatteryHealthCheck;

#[async_trait]
impl DoctorCheck for BatteryHealthCheck {
    fn name(&self) -> &'static str {
        "Battery life"
    }

    fn id(&self) -> &'static str {
        "battery-life"
    }

    async fn execute(&self) -> CheckStatus {
        BatteryCheck::execute(self).await
    }
}

impl BatteryCheck for BatteryHealthCheck {
    fn check_parameter(&self, mut batteries: Vec<Battery>) -> CheckStatus {
        batteries.sort_by(|b1, b2| {
            b1.state_of_health()
                .value
                .total_cmp(&b2.state_of_health().value)
        });
        let lowest = batteries.first().unwrap();
        let lowest_health = lowest.state_of_health();
        let percent = (lowest_health.value * 100.0) as u64;
        if percent < 90 {
            CheckStatus::Warning {
                message: "Battery health degraded".into(),
                fixable: false,
            }
        } else {
            CheckStatus::Pass(format!("Battery life: {}%", percent))
        }
    }
}
