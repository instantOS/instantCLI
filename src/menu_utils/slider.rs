use anyhow::{Context, Result, anyhow};
use std::process::Command;

/// Describes an external command to execute whenever the slider value changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliderCommand {
    program: String,
    args: Vec<String>,
}

impl SliderCommand {
    /// Create a new slider command from a sequence of arguments where the first
    /// element is treated as the program and the rest as fixed arguments.
    pub fn from_argv(arguments: &[String]) -> Result<Option<Self>> {
        if arguments.is_empty() {
            return Ok(None);
        }

        let program = arguments
            .first()
            .cloned()
            .context("Command requires at least one argument specifying the program")?;

        let args = arguments[1..].to_vec();

        Ok(Some(Self { program, args }))
    }

    /// Execute the command, appending the provided value as the final argument.
    pub fn spawn_with_value(&self, value: i64) -> Result<()> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command.arg(value.to_string());

        command
            .spawn()
            .with_context(|| format!("Failed to execute slider command '{}'", self.program))?;

        Ok(())
    }

    /// Raw components used for serialization when communicating with the menu server.
    pub fn components(&self) -> (&str, &[String]) {
        (&self.program, &self.args)
    }

    /// Reconstruct the command from raw components.
    pub fn from_components(program: String, args: Vec<String>) -> Self {
        Self { program, args }
    }
}

/// Configuration for the slider TUI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliderConfig {
    pub min: i64,
    pub max: i64,
    pub value: i64,
    pub step: i64,
    pub large_step: i64,
    pub label: Option<String>,
    pub command: Option<SliderCommand>,
}

impl SliderConfig {
    /// Create a slider configuration ensuring sane defaults and value clamping.
    pub fn new(
        min: i64,
        max: i64,
        value: Option<i64>,
        step: Option<i64>,
        large_step: Option<i64>,
        label: Option<String>,
        command: Option<SliderCommand>,
    ) -> Result<Self> {
        if min >= max {
            return Err(anyhow!("Slider minimum ({min}) must be less than maximum ({max})"));
        }

        let range = max - min;

        let default_small = if range >= 100 { 1 } else { 1.max(range / 100) };
        let default_large = (range / 10).max(5);

        let step = step.unwrap_or(default_small).max(1);
        let large_step = large_step.unwrap_or(default_large).max(step);

        let mut value = value.unwrap_or(min + range / 2);
        value = value.clamp(min, max);

        Ok(Self {
            min,
            max,
            value,
            step,
            large_step,
            label,
            command,
        })
    }

    /// Clamp an arbitrary value into the slider range.
    pub fn clamp(&self, value: i64) -> i64 {
        value.clamp(self.min, self.max)
    }

    /// Update the stored value while clamping to range.
    pub fn set_value(&mut self, value: i64) {
        self.value = self.clamp(value);
    }

    /// Calculate the slider range as a floating-point value for rendering.
    pub fn ratio(&self) -> f64 {
        if self.max == self.min {
            return 0.0;
        }

        let offset = self.value - self.min;
        (offset as f64) / ((self.max - self.min) as f64)
    }

    /// Adjust the value by a delta and return true if it changed.
    pub fn apply_delta(&mut self, delta: i64) -> bool {
        let new_value = self.clamp(self.value.saturating_add(delta));
        if new_value != self.value {
            self.value = new_value;
            return true;
        }
        false
    }

    /// Snap the value to the provided fraction of the range (0.0..=1.0).
    pub fn snap_to_fraction(&mut self, fraction: f64) -> bool {
        let fraction = fraction.clamp(0.0, 1.0);
        let range = (self.max - self.min) as f64;
        let snapped = self.min as f64 + range * fraction;
        let new_value = self.clamp(snapped.round() as i64);
        if new_value != self.value {
            self.value = new_value;
            return true;
        }
        false
    }
}
