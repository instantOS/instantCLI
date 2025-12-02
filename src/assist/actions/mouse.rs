use crate::common::compositor::CompositorType;
use crate::menu::client::MenuClient;
use crate::menu::protocol::SliderRequest;
use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

pub fn mouse_speed_slider() -> Result<()> {
    let compositor = CompositorType::detect();
    if compositor != CompositorType::Sway {
        anyhow::bail!(
            "Mouse speed adjustment is currently only supported on Sway. Detected: {}",
            compositor.name()
        );
    }

    // Detect current speed
    let current_speed = get_sway_mouse_speed().unwrap_or(0.0);

    // Map -1.0..1.0 to 0..100
    // speed = (value / 50.0) - 1.0
    // value = (speed + 1.0) * 50.0
    let initial_value = ((current_speed + 1.0) * 50.0) as i64;

    let client = MenuClient::new();
    client.ensure_server_running()?;

    // We need a command that the slider can execute.
    // We'll use "ins assist mouse-set"
    let current_exe = std::env::current_exe()?;
    let program = current_exe.to_string_lossy().to_string();
    let args = vec![program, "assist".to_string(), "mouse-set".to_string()];

    let request = SliderRequest {
        min: 0,
        max: 100,
        value: Some(initial_value),
        step: Some(1),
        big_step: Some(10),
        label: Some("Mouse Speed".to_string()),
        command: args,
    };

    client.slide(request)?;
    Ok(())
}

pub fn set_mouse_speed(value: i64) -> Result<()> {
    // Map 0..100 to -1.0..1.0
    let speed = (value as f64 / 50.0) - 1.0;

    // Clamp to -1.0..1.0 just in case
    let speed = speed.clamp(-1.0, 1.0);

    // Apply to sway
    // swaymsg input type:pointer pointer_accel <value>
    // Need to pass as a single argument to avoid swaymsg interpreting negative values as options
    let sway_command = format!("input type:pointer pointer_accel {}", speed);

    Command::new("swaymsg")
        .arg(sway_command)
        .output()
        .context("Failed to set mouse speed")?;

    Ok(())
}

fn get_sway_mouse_speed() -> Result<f64> {
    let output = Command::new("swaymsg")
        .arg("-t")
        .arg("get_inputs")
        .output()
        .context("Failed to get inputs")?;

    let json: Value = serde_json::from_slice(&output.stdout)?;

    // Find first pointer device that actually has accel_speed property
    if let Some(inputs) = json.as_array() {
        for input in inputs {
            if let Some(type_) = input.get("type").and_then(|v| v.as_str()) {
                if type_ == "pointer" {
                    if let Some(libinput) = input.get("libinput") {
                        // Only consider devices that have accel_speed property
                        if let Some(accel) = libinput.get("accel_speed") {
                            if let Some(speed) = accel.as_f64() {
                                return Ok(speed);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(0.0) // Default
}
