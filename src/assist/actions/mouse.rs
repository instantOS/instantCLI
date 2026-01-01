use crate::common::compositor::CompositorType;
use crate::menu::client::MenuClient;
use crate::menu::protocol::SliderRequest;
use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

pub fn mouse_speed_slider() -> Result<()> {
    run_mouse_speed_slider(None)?;
    Ok(())
}

pub fn run_mouse_speed_slider(initial_value: Option<i64>) -> Result<Option<i64>> {
    let compositor = CompositorType::detect();

    // Set accel profile based on compositor
    match compositor {
        CompositorType::Sway => {
            Command::new("swaymsg")
                .arg("input type:pointer accel_profile flat")
                .output()
                .context("Failed to set mouse accel profile to flat")?;
        }
        CompositorType::Gnome => {}
        _ if compositor.is_x11() => {}
        _ => {
            anyhow::bail!(
                "Mouse speed adjustment is only supported on Sway, X11, and Gnome. Detected: {}",
                compositor.name()
            );
        }
    }

    let start_value = if let Some(v) = initial_value {
        v
    } else {
        // Detect current speed based on compositor
        let current_speed = match compositor {
            CompositorType::Sway => get_sway_mouse_speed().unwrap_or(0.0),
            CompositorType::Gnome => get_gnome_mouse_speed().unwrap_or(0.0),
            _ if compositor.is_x11() => get_x11_mouse_speed().unwrap_or(0.0),
            _ => 0.0,
        };

        // Map -1.0..1.0 to 0..100
        // speed = (value / 50.0) - 1.0
        // value = (speed + 1.0) * 50.0
        ((current_speed + 1.0) * 50.0) as i64
    };

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
        value: Some(start_value),
        step: Some(1),
        big_step: Some(10),
        label: Some("Mouse Speed".to_string()),
        command: args,
    };

    client.slide(request)
}

pub fn set_mouse_speed(value: i64) -> Result<()> {
    // Map 0..100 to -1.0..1.0
    let speed = (value as f64 / 50.0) - 1.0;

    // Clamp to -1.0..1.0 just in case
    let speed = speed.clamp(-1.0, 1.0);

    let compositor = CompositorType::detect();

    match compositor {
        CompositorType::Sway => {
            let sway_command = format!("input type:pointer pointer_accel {}", speed);

            Command::new("swaymsg")
                .arg(sway_command)
                .output()
                .context("Failed to set mouse speed")?;
        }
        CompositorType::Gnome => {
            Command::new("gsettings")
                .args(["set", "org.gnome.desktop.peripherals.mouse", "speed", &speed.to_string()])
                .output()
                .context("Failed to set mouse speed")?;
            Command::new("gsettings")
                .args(["set", "org.gnome.desktop.peripherals.touchpad", "speed", &speed.to_string()])
                .output()
                .context("Failed to set touchpad speed")?;
        }
        _ if compositor.is_x11() => {
            set_x11_mouse_speed(speed)?;
        }
        _ => {
            anyhow::bail!(
                "Mouse speed adjustment is only supported on Sway, X11, and Gnome. Detected: {}",
                compositor.name()
            );
        }
    }

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
            if let Some(type_) = input.get("type").and_then(|v| v.as_str())
                && type_ == "pointer"
                && let Some(libinput) = input.get("libinput")
            {
                // Only consider devices that have accel_speed property
                if let Some(accel) = libinput.get("accel_speed")
                    && let Some(speed) = accel.as_f64()
                {
                    return Ok(speed);
                }
            }
        }
    }

    Ok(0.0) // Default
}

fn get_x11_mouse_devices() -> Result<Vec<String>> {
    let output = Command::new("xinput")
        .arg("list")
        .output()
        .context("Failed to run xinput list")?;

    let output_str = String::from_utf8_lossy(&output.stdout);

    // Extract device IDs
    let mut device_ids = Vec::new();
    for line in output_str.lines() {
        if let Some(id_str) = line.split("id=").nth(1)
            && let Some(id) = id_str.split_whitespace().next()
        {
            device_ids.push(id.to_string());
        }
    }

    // Filter to devices that support libinput Accel Speed
    let mut mouse_devices = Vec::new();
    for id in device_ids {
        let props_output = Command::new("xinput").args(["list-props", &id]).output();

        if let Ok(props) = props_output {
            let props_str = String::from_utf8_lossy(&props.stdout);
            if props_str.contains("libinput Accel Speed") {
                mouse_devices.push(id);
            }
        }
    }

    Ok(mouse_devices)
}

fn get_x11_mouse_speed() -> Result<f64> {
    let devices = get_x11_mouse_devices()?;

    if let Some(first_device) = devices.first() {
        let output = Command::new("xinput")
            .args(["list-props", first_device])
            .output()
            .context("Failed to get xinput properties")?;

        let output_str = String::from_utf8_lossy(&output.stdout);

        // Parse "libinput Accel Speed (nnn):	-0.400000" format
        for line in output_str.lines() {
            if line.contains("libinput Accel Speed")
                && let Some(value_str) = line.split(':').nth(1)
                && let Ok(speed) = value_str.trim().parse::<f64>()
            {
                return Ok(speed);
            }
        }
    }

    Ok(0.0) // Default
}

fn set_x11_mouse_speed(speed: f64) -> Result<()> {
    let devices = get_x11_mouse_devices()?;

    if devices.is_empty() {
        anyhow::bail!("No mouse devices with libinput support found");
    }

    for device_id in devices {
        Command::new("xinput")
            .args([
                "set-prop",
                &device_id,
                "libinput Accel Speed",
                &speed.to_string(),
            ])
            .output()
            .with_context(|| format!("Failed to set mouse speed for device {}", device_id))?;
    }

    Ok(())
}

fn get_gnome_mouse_speed() -> Result<f64> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.peripherals.touchpad", "speed"])
        .output()
        .context("Failed to get GNOME touchpad speed")?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    let speed = output_str.trim().parse::<f64>().unwrap_or(0.0);

    if speed != 0.0 {
        return Ok(speed);
    }

    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.peripherals.mouse", "speed"])
        .output()
        .context("Failed to get GNOME mouse speed")?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    let speed = output_str.trim().parse::<f64>().unwrap_or(0.0);

    Ok(speed)
}
