use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::Command;

use crate::assist::utils::show_notification;
use crate::common::compositor::CompositorType;
use crate::common::display::SwayDisplayProvider;
use crate::common::display_server::DisplayServer;
use crate::menu::client::MenuClient;
use crate::menu::protocol::{FzfPreview, SerializableMenuItem};

/// Represents a display output for mirroring operations
#[derive(Debug, Clone)]
struct Output {
    name: String,
    description: String,
    active: bool,
    geometry: Option<String>,
}

/// Actions available in the mirror menu
#[derive(Debug, Clone, Copy, PartialEq)]
enum MirrorAction {
    Stop,
    Mirror,
}

impl std::fmt::Display for MirrorAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MirrorAction::Stop => write!(f, "stop"),
            MirrorAction::Mirror => write!(f, "mirror"),
        }
    }
}

impl MirrorAction {
    fn from_str(s: &str) -> Self {
        match s {
            "stop" => MirrorAction::Stop,
            _ => MirrorAction::Mirror,
        }
    }
}

/// User selection from the mirror menu
#[derive(Debug, Clone)]
struct MirrorSelection {
    action: MirrorAction,
    output_name: Option<String>,
}

// ============================================================================
// Public API
// ============================================================================

pub fn mirror_output() -> Result<()> {
    let display_server = DisplayServer::detect();

    match display_server {
        DisplayServer::Wayland => wayland::mirror(),
        DisplayServer::X11 => x11::mirror(),
        _ => anyhow::bail!("Unsupported display server for screen mirroring"),
    }
}

// ============================================================================
// Wayland Implementation
// ============================================================================

mod wayland {
    use super::*;

    pub fn mirror() -> Result<()> {
        validate_compositor()?;

        let outputs = fetch_outputs()?;
        if outputs.is_empty() {
            show_notification("Mirror", "No outputs detected")?;
            return Ok(());
        }
        if outputs.len() == 1 && !is_wl_mirror_running() {
            show_notification("Mirror", "Only one display connected - nothing to mirror")?;
            return Ok(());
        }

        let menu_items = build_menu(&outputs);
        let selection = show_menu("Screen Mirroring (Wayland)", menu_items)?;

        match selection {
            Some(sel) => handle_selection(sel),
            None => Ok(()),
        }
    }

    fn validate_compositor() -> Result<()> {
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::Sway) {
            anyhow::bail!(
                "wl-mirror assist currently supports Sway only. Detected: {}.",
                compositor.name()
            );
        }
        Ok(())
    }

    fn fetch_outputs() -> Result<Vec<Output>> {
        let sway_outputs =
            SwayDisplayProvider::get_outputs_sync().context("Failed to query Sway outputs")?;

        Ok(sway_outputs
            .into_iter()
            .map(|o| Output {
                name: o.name.clone(),
                description: o.display_label(),
                active: true,
                geometry: None,
            })
            .collect())
    }

    fn build_menu(outputs: &[Output]) -> Vec<SerializableMenuItem> {
        let mut items = Vec::new();

        if is_wl_mirror_running() {
            items.push(create_menu_item(
                "Stop Mirroring (Close wl-mirror)",
                MirrorAction::Stop,
                None,
            ));
        }

        for output in outputs {
            items.push(create_menu_item(
                &format!("Mirror {}", output.description),
                MirrorAction::Mirror,
                Some(&output.name),
            ));
        }

        items
    }

    fn handle_selection(selection: MirrorSelection) -> Result<()> {
        match selection.action {
            MirrorAction::Stop => stop_mirroring(),
            MirrorAction::Mirror => {
                let output = selection
                    .output_name
                    .context("No output specified for mirroring")?;
                start_mirroring(&output)
            }
        }
    }

    fn stop_mirroring() -> Result<()> {
        show_notification("Mirror", "Stopping wl-mirror...")?;
        Command::new("pkill")
            .arg("-x")
            .arg("wl-mirror")
            .spawn()
            .context("Failed to kill wl-mirror")?;
        Ok(())
    }

    fn start_mirroring(output: &str) -> Result<()> {
        Command::new("wl-mirror")
            .arg(output)
            .spawn()
            .context("Failed to launch wl-mirror")?;
        Ok(())
    }

    fn is_wl_mirror_running() -> bool {
        Command::new("pgrep")
            .arg("-x")
            .arg("wl-mirror")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ============================================================================
// X11 Implementation
// ============================================================================

mod x11 {
    use super::*;

    pub fn mirror() -> Result<()> {
        let outputs = fetch_outputs()?;
        if outputs.is_empty() {
            show_notification("Mirror", "No connected outputs detected")?;
            return Ok(());
        }
        if outputs.len() == 1 {
            show_notification("Mirror", "Only one display connected - nothing to mirror")?;
            return Ok(());
        }

        let is_mirrored = detect_mirror_state(&outputs);
        let menu_items = build_menu(&outputs, is_mirrored);
        let selection = show_menu("Screen Mirroring (X11)", menu_items)?;

        match selection {
            Some(sel) => handle_selection(sel, &outputs),
            None => Ok(()),
        }
    }

    fn detect_mirror_state(outputs: &[Output]) -> bool {
        let mut seen_geometries = HashMap::new();

        for output in outputs {
            if output.active
                && let Some(geo) = &output.geometry
            {
                if seen_geometries.contains_key(geo) {
                    return true;
                }
                seen_geometries.insert(geo.clone(), true);
            }
        }

        false
    }

    fn build_menu(outputs: &[Output], is_mirrored: bool) -> Vec<SerializableMenuItem> {
        let mut items = Vec::new();

        let stop_label = if is_mirrored {
            "Restore Extended Mode (Mirrored Detected)"
        } else {
            "Stop Mirroring (Restore Profile)"
        };
        items.push(create_menu_item(stop_label, MirrorAction::Stop, None));

        for output in outputs {
            let label = if output.active {
                format!("{} (Active)", output.name)
            } else {
                output.name.clone()
            };
            items.push(create_menu_item(
                &format!("Mirror {}", label),
                MirrorAction::Mirror,
                Some(&output.name),
            ));
        }

        items
    }

    fn handle_selection(selection: MirrorSelection, outputs: &[Output]) -> Result<()> {
        match selection.action {
            MirrorAction::Stop => stop_mirroring(),
            MirrorAction::Mirror => {
                let source = selection
                    .output_name
                    .context("No source output specified")?;
                mirror_to_target(&source, outputs)
            }
        }
    }

    fn stop_mirroring() -> Result<()> {
        show_notification("Mirror", "Restoring display profile...")?;
        Command::new("autorandr")
            .arg("--change")
            .spawn()
            .context("Failed to run autorandr")?;
        Ok(())
    }

    fn mirror_to_target(source: &str, outputs: &[Output]) -> Result<()> {
        let targets: Vec<&Output> = outputs.iter().filter(|o| o.name != source).collect();

        if targets.is_empty() {
            show_notification("Mirror", "No other outputs to mirror to")?;
            return Ok(());
        }

        let target_name = if targets.len() == 1 {
            targets[0].name.clone()
        } else {
            match select_target(targets)? {
                Some(name) => name,
                None => return Ok(()),
            }
        };

        execute_mirror_command(source, &target_name)
    }

    fn select_target(targets: Vec<&Output>) -> Result<Option<String>> {
        let items: Vec<SerializableMenuItem> = targets
            .iter()
            .map(|output| create_menu_item(&output.name, MirrorAction::Mirror, Some(&output.name)))
            .collect();

        let selection = show_menu("Mirror TO which display?", items)?;
        Ok(selection.and_then(|s| s.output_name))
    }

    fn execute_mirror_command(source: &str, target: &str) -> Result<()> {
        show_notification("Mirror", &format!("Mirroring {} to {}", source, target))?;

        Command::new("xrandr")
            .args(["--output", target, "--same-as", source, "--auto"])
            .spawn()
            .context("Failed to run xrandr")?;

        Ok(())
    }
}

// ============================================================================
// Output Fetching (X11)
// ============================================================================

fn fetch_outputs() -> Result<Vec<Output>> {
    let output = Command::new("xrandr")
        .arg("--query")
        .output()
        .context("Failed to run xrandr")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut outputs = Vec::new();

    for line in stdout.lines() {
        if line.contains(" connected")
            && let Some(output) = parse_xrandr_line(line)
        {
            outputs.push(output);
        }
    }

    Ok(outputs)
}

fn parse_xrandr_line(line: &str) -> Option<Output> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let name = parts.first()?.to_string();

    let mut geometry = None;
    let mut description = "Connected".to_string();

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

    Some(Output {
        name,
        description,
        active: line.contains("connected primary") || line.contains('+'),
        geometry,
    })
}

// ============================================================================
// Common UI Helpers
// ============================================================================

fn create_menu_item(
    display_text: &str,
    action: MirrorAction,
    output_name: Option<&str>,
) -> SerializableMenuItem {
    let mut metadata = HashMap::new();
    metadata.insert("action".to_string(), action.to_string());

    if let Some(name) = output_name {
        metadata.insert("output".to_string(), name.to_string());
    }

    SerializableMenuItem {
        display_text: display_text.to_string(),
        preview: FzfPreview::None,
        metadata: Some(metadata),
    }
}

fn show_menu(prompt: &str, items: Vec<SerializableMenuItem>) -> Result<Option<MirrorSelection>> {
    let client = MenuClient::new();
    let selected = client.choice(prompt.to_string(), items, false)?;

    let item = match selected.first() {
        Some(item) => item,
        None => return Ok(None),
    };

    parse_menu_selection(item)
}

fn parse_menu_selection(item: &SerializableMenuItem) -> Result<Option<MirrorSelection>> {
    let metadata = item.metadata.as_ref().context("No metadata in selection")?;

    let action = metadata
        .get("action")
        .map(|s| MirrorAction::from_str(s))
        .unwrap_or(MirrorAction::Mirror);

    let output_name = metadata.get("output").cloned();

    Ok(Some(MirrorSelection {
        action,
        output_name,
    }))
}
