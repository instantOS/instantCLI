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

        let program = arguments[0].clone();
        let args = arguments[1..].to_vec();

        Ok(Some(Self { program, args }))
    }

    /// Execute the command, appending the provided value as the final argument.
    pub fn spawn_with_value(&self, value: i64) -> Result<()> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command.arg(value.to_string());

        // Redirect stdout and stderr to prevent interference with TUI
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());

        let mut child = command
            .spawn()
            .with_context(|| format!("Failed to execute slider command '{}'", self.program))?;

        // Reap the child process in a background thread to avoid zombie processes
        std::thread::spawn(move || {
            let _ = child.wait();
        });

        Ok(())
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

/// Builder for constructing a `SliderConfig` with sensible defaults.
pub struct SliderConfigBuilder {
    min: i64,
    max: i64,
    value: Option<i64>,
    step: Option<i64>,
    large_step: Option<i64>,
    label: Option<String>,
    command: Option<SliderCommand>,
}

impl SliderConfigBuilder {
    pub fn min(mut self, min: i64) -> Self {
        self.min = min;
        self
    }

    pub fn max(mut self, max: i64) -> Self {
        self.max = max;
        self
    }

    pub fn value(mut self, value: Option<i64>) -> Self {
        self.value = value;
        self
    }

    pub fn step(mut self, step: Option<i64>) -> Self {
        self.step = step;
        self
    }

    pub fn large_step(mut self, large_step: Option<i64>) -> Self {
        self.large_step = large_step;
        self
    }

    pub fn label(mut self, label: Option<String>) -> Self {
        self.label = label;
        self
    }

    pub fn command(mut self, command: Option<SliderCommand>) -> Self {
        self.command = command;
        self
    }

    /// Build the slider configuration, validating constraints and applying defaults.
    pub fn build(self) -> Result<SliderConfig> {
        if self.min >= self.max {
            return Err(anyhow!(
                "Slider minimum ({}) must be less than maximum ({})",
                self.min,
                self.max
            ));
        }

        let range = self.max - self.min;
        let default_large = (range / 10).max(5);

        let step = self.step.unwrap_or(1).max(1);
        let large_step = self.large_step.unwrap_or(default_large).max(step);

        let mut value = self.value.unwrap_or(self.min + range / 2);
        value = value.clamp(self.min, self.max);

        Ok(SliderConfig {
            min: self.min,
            max: self.max,
            value,
            step,
            large_step,
            label: self.label,
            command: self.command,
        })
    }
}

impl SliderConfig {
    /// Create a builder for configuring a slider.
    pub fn builder() -> SliderConfigBuilder {
        SliderConfigBuilder {
            min: 0,
            max: 100,
            value: None,
            step: None,
            large_step: None,
            label: None,
            command: None,
        }
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
