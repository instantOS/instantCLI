use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::Command;

use crate::assist::utils::show_notification;
use crate::common::compositor::CompositorType;
use crate::common::display::SwayDisplayProvider;
use crate::common::display_server::DisplayServer;
use crate::menu::client::MenuClient;
use crate::menu::protocol::{FzfPreview, SerializableMenuItem};

struct MirrorOutput {
    name: String,
    description: String,
    active: bool,
    geometry: Option<String>,
}

pub fn mirror_output() -> Result<()> {
    let display_server = DisplayServer::detect();

    match display_server {
        DisplayServer::Wayland => mirror_output_wayland(),
        DisplayServer::X11 => mirror_output_x11(),
        _ => {
            anyhow::bail!("Unsupported display server for screen mirroring");
        }
    }
}

fn is_wl_mirror_running() -> bool {
    Command::new("pgrep")
        .arg("-x")
        .arg("wl-mirror")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn mirror_output_wayland() -> Result<()> {
    let compositor = CompositorType::detect();
    if !matches!(compositor, CompositorType::Sway) {
        anyhow::bail!(
            "wl-mirror assist currently supports Sway only. Detected: {}.",
            compositor.name()
        );
    }

    let sway_outputs =
        SwayDisplayProvider::get_outputs_sync().context("Failed to query Sway outputs")?;

    if sway_outputs.is_empty() {
        show_notification("Mirror", "No outputs detected")?;
        return Ok(());
    }

    let outputs: Vec<MirrorOutput> = sway_outputs
        .into_iter()
        .map(|o| MirrorOutput {
            name: o.name.clone(),
            description: o.display_label(),
            active: true,
            geometry: None, // Geometry not needed for Wayland/wl-mirror detection logic
        })
        .collect();

    let mut options = Vec::new();

    // If wl-mirror is already running, offer to stop it (restore extended/original state)
    if is_wl_mirror_running() {
        options.push(SerializableMenuItem {
            display_text: "Stop Mirroring (Close wl-mirror)".to_string(),
            preview: FzfPreview::None,
            metadata: Some(HashMap::from([("action".to_string(), "stop".to_string())])),
        });
    }

    for output in &outputs {
        options.push(SerializableMenuItem {
            display_text: format!("Mirror {}", output.description),
            preview: FzfPreview::None,
            metadata: Some(HashMap::from([
                ("action".to_string(), "mirror".to_string()),
                ("output".to_string(), output.name.clone()),
            ])),
        });
    }

    let client = MenuClient::new();
    let selected = client.choice("Screen Mirroring (Wayland)".to_string(), options, false)?;

    let item = match selected.first() {
        Some(item) => item,
        None => return Ok(()),
    };

    let metadata = item.metadata.as_ref().context("No metadata in selection")?;
    let action = metadata
        .get("action")
        .map(|s| s.as_str())
        .unwrap_or("mirror");

    if action == "stop" {
        show_notification("Mirror", "Stopping wl-mirror...")?;
        Command::new("pkill")
            .arg("-x")
            .arg("wl-mirror")
            .spawn()
            .context("Failed to kill wl-mirror")?;
        return Ok(());
    }

    let output_name = metadata.get("output").context("No output specified")?;

    Command::new("wl-mirror")
        .arg(output_name)
        .spawn()
        .context("Failed to launch wl-mirror")?;

    Ok(())
}

fn mirror_output_x11() -> Result<()> {
    let outputs = get_x11_outputs()?;
    if outputs.is_empty() {
        show_notification("Mirror", "No connected outputs detected")?;
        return Ok(());
    }

    // Detect if mirroring is active by checking for duplicate geometries among active outputs
    let mut active_geometries = HashMap::new();
    let mut is_mirrored = false;
    for output in &outputs {
        if output.active {
            if let Some(geo) = &output.geometry {
                // If we've seen this geometry before, it's a mirror (overlap)
                if active_geometries.contains_key(geo) {
                    is_mirrored = true;
                }
                active_geometries.insert(geo.clone(), true);
            }
        }
    }

    let mut options = Vec::new();

    let stop_label = if is_mirrored {
        "Restore Extended Mode (Mirrored Detected)"
    } else {
        "Stop Mirroring (Restore Profile)"
    };

    options.push(SerializableMenuItem {
        display_text: stop_label.to_string(),
        preview: FzfPreview::None,
        metadata: Some(HashMap::from([("action".to_string(), "stop".to_string())])),
    });

    for output in &outputs {
        let label = if output.active {
            format!("{} (Active)", output.name)
        } else {
            output.name.clone()
        };

        options.push(SerializableMenuItem {
            display_text: format!("Mirror {}", label),
            preview: FzfPreview::None,
            metadata: Some(HashMap::from([
                ("action".to_string(), "mirror".to_string()),
                ("output".to_string(), output.name.clone()),
            ])),
        });
    }

    let client = MenuClient::new();
    let selected = client.choice("Screen Mirroring (X11)".to_string(), options, false)?;

    let item = match selected.first() {
        Some(item) => item,
        None => return Ok(()),
    };

    let metadata = item.metadata.as_ref().context("No metadata in selection")?;
    let action = metadata
        .get("action")
        .map(|s| s.as_str())
        .unwrap_or("mirror");

    if action == "stop" {
        show_notification("Mirror", "Restoring display profile...")?;
        Command::new("autorandr")
            .arg("--change")
            .spawn()
            .context("Failed to run autorandr")?;
        return Ok(());
    }

    let source = metadata.get("output").context("No output specified")?;

    // Now select target(s)
    let potential_targets: Vec<MirrorOutput> = outputs
        .iter()
        .filter(|o| &o.name != source)
        .map(|o| MirrorOutput {
            name: o.name.clone(),
            description: o.description.clone(),
            active: o.active,
            geometry: o.geometry.clone(),
        })
        .collect();

    if potential_targets.is_empty() {
        show_notification("Mirror", "No other outputs to mirror to")?;
        return Ok(());
    }

    let target = if potential_targets.len() == 1 {
        potential_targets[0].name.clone()
    } else {
        match select_output(&potential_targets, "Mirror TO which display?")? {
            Some(name) => name,
            None => return Ok(()),
        }
    };

    show_notification("Mirror", &format!("Mirroring {} to {}", source, target))?;

    // Execute xrandr command
    // xrandr --output <Target> --same-as <Source> --auto
    Command::new("xrandr")
        .args(["--output", &target, "--same-as", source, "--auto"])
        .spawn()
        .context("Failed to run xrandr")?;

    Ok(())
}

fn get_x11_outputs() -> Result<Vec<MirrorOutput>> {
    let output = Command::new("xrandr")
        .arg("--query")
        .output()
        .context("Failed to run xrandr")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut outputs = Vec::new();

    for line in stdout.lines() {
        if line.contains(" connected") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(name) = parts.first() {
                // Parse line to find geometry
                // Line format: "HDMI-1 connected primary 1920x1080+0+0 ..." or "HDMI-1 connected 1920x1080+0+0 ..."
                // Geometry token usually contains "+".

                let mut geometry = None;
                let mut description = "Connected".to_string();
                let _is_primary = line.contains(" primary ");

                // Identify the geometry token
                // It's usually the token after "connected" (and optionally "primary")
                // And it should contain "+" (e.g., 1920x1080+0+0)

                for part in &parts {
                    if part.contains('x') && part.contains('+') {
                        geometry = Some(part.to_string());
                        description = part.to_string();
                        break;
                    }
                }

                if description == "Connected" && !line.contains('+') {
                    description = "Connected (Inactive)".to_string();
                }

                outputs.push(MirrorOutput {
                    name: name.to_string(),
                    description,
                    active: line.contains("connected primary") || line.contains('+'),
                    geometry,
                });
            }
        }
    }

    Ok(outputs)
}

fn select_output(outputs: &[MirrorOutput], prompt: &str) -> Result<Option<String>> {
    let items: Vec<SerializableMenuItem> = outputs
        .iter()
        .map(|output| {
            let mut metadata = HashMap::new();
            metadata.insert("output".to_string(), output.name.clone());

            let label = if !output.description.is_empty() {
                if output.description != output.name {
                    output.description.clone()
                } else {
                    output.name.clone()
                }
            } else {
                output.name.clone()
            };

            SerializableMenuItem {
                display_text: label,
                preview: FzfPreview::None,
                metadata: Some(metadata),
            }
        })
        .collect();

    let client = MenuClient::new();
    let selected = client.choice(prompt.to_string(), items, false)?;

    let item = match selected.first() {
        Some(item) => item,
        None => return Ok(None),
    };

    let output_name = item
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("output").cloned())
        .or_else(|| {
            item.display_text
                .split_whitespace()
                .next()
                .map(|s| s.to_string())
        });

    Ok(output_name)
}
