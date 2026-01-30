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
            active: true, // Sway outputs from this provider are generally active
        })
        .collect();

    let output_name = if outputs.len() == 1 {
        outputs[0].name.clone()
    } else {
        match select_output(&outputs, "Mirror which output?")? {
            Some(name) => name,
            None => return Ok(()),
        }
    };

    Command::new("wl-mirror")
        .arg(&output_name)
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

    // Add special option to stop mirroring (revert to autorandr profile)
    let mut options = Vec::new();
    options.push(SerializableMenuItem {
        display_text: "Stop Mirroring (Restore Profile)".to_string(),
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
    let selected = client.choice("Screen Mirroring".to_string(), options, false)?;

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
                // Try to parse resolution if active, otherwise just "connected"
                let description = if let Some(res) = parts.get(2) {
                    if res.contains('x') && res.contains('+') {
                        res.to_string()
                    } else {
                        "Connected (Inactive)".to_string()
                    }
                } else {
                    "Connected".to_string()
                };

                outputs.push(MirrorOutput {
                    name: name.to_string(),
                    description,
                    active: line.contains("connected primary") || line.contains("+"),
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
