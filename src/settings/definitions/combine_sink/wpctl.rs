use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::settings::context::SettingsContext;

use super::INS_COMBINED_SINK_PREFIX;
use super::model::SinkInfo;

/// Run wpctl status and parse the Sinks section
pub(super) fn list_sinks() -> Result<Vec<SinkInfo>> {
    let output = Command::new("wpctl")
        .arg("status")
        .output()
        .context("Failed to run wpctl status")?;

    if !output.status.success() {
        bail!("wpctl status failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_wpctl_status(&stdout)
}

/// Parse wpctl status output to extract sinks
pub(super) fn parse_wpctl_status(output: &str) -> Result<Vec<SinkInfo>> {
    let mut sinks = Vec::new();
    let mut in_sinks_section = false;

    for line in output.lines() {
        // Detect start of Sinks section
        if line.contains("├─ Sinks:") || line.contains("└─ Sinks:") {
            in_sinks_section = true;
            continue;
        }

        // Detect end of Sinks section (next section starts)
        if in_sinks_section && (line.contains("├─") || line.contains("└─")) && !line.contains("│")
        {
            // Check if this is a different section
            if line.contains("Sources:") || line.contains("Filters:") || line.contains("Streams:") {
                break;
            }
        }

        if in_sinks_section {
            // Parse sink lines like:
            // │  *   78. Radeon High Definition Audio Controller Digitales Stereo (HDMI) [vol: 0.95]
            // │      48. DualSense wireless controller (PS5) 0 [vol: 1.00]
            if let Some(sink) = parse_sink_line(line) {
                sinks.push(sink);
            }
        }
    }

    if sinks.is_empty() {
        bail!("No audio sinks found. Is PipeWire running?");
    }

    Ok(sinks)
}

/// Parse a single sink line from wpctl status
fn parse_sink_line(line: &str) -> Option<SinkInfo> {
    // Remove tree drawing characters
    let cleaned = line.replace(['│', '├', '└', '─'], "").trim().to_string();

    // Check if this is the default sink (marked with *)
    let is_default = cleaned.contains('*');

    // Extract ID and description
    // Format: "*   78. Description [vol: 0.95]" or "48. Description [vol: 1.00]"
    let without_star = cleaned.replace('*', "").trim().to_string();

    // Find the ID (number followed by dot)
    let parts: Vec<&str> = without_star.splitn(2, ". ").collect();
    if parts.len() < 2 {
        return None;
    }

    let id = parts[0].trim().to_string();
    let rest = parts[1];

    // Extract volume if present
    let (description, volume) = if let Some(vol_start) = rest.find(" [vol:") {
        let desc = rest[..vol_start].trim().to_string();
        let vol = rest[vol_start..]
            .trim_start_matches(" [vol:")
            .trim_end_matches(']')
            .trim()
            .to_string();
        (desc, Some(vol))
    } else {
        (rest.trim().to_string(), None)
    };

    // Get node name by inspecting the sink
    let node_name = get_node_name(&id).unwrap_or_else(|_| format!("sink_{}", id));

    Some(SinkInfo {
        id,
        name: description.clone(),
        node_name,
        description,
        volume,
        is_default,
    })
}

/// Get the node.name property for a sink using wpctl inspect
fn get_node_name(sink_id: &str) -> Result<String> {
    let output = Command::new("wpctl")
        .args(["inspect", sink_id])
        .output()
        .context("Failed to run wpctl inspect")?;

    if !output.status.success() {
        bail!("wpctl inspect {} failed", sink_id);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse node.name from output
    // Handle both formats:
    //   node.name = "value"
    // * node.name = "value" (marked as default)
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        // Remove leading "* " if present (indicates default property)
        let without_star = if let Some(stripped) = trimmed.strip_prefix("* ") {
            stripped
        } else {
            trimmed
        };

        if let Some(value) = without_star.strip_prefix("node.name = \"")
            && let Some(end) = value.find('"')
        {
            return Ok(value[..end].to_string());
        }
    }

    bail!(
        "node.name not found in wpctl inspect output for sink {}",
        sink_id
    )
}

/// Find the ID of the combined sink from wpctl status by looking for our prefix
/// The combined sink may appear in Sinks or Filters section depending on PipeWire version
fn find_combined_sink_id() -> Result<String> {
    let output = Command::new("wpctl")
        .arg("status")
        .output()
        .context("Failed to run wpctl status")?;

    if !output.status.success() {
        bail!("wpctl status failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Search all lines for our prefix - it can appear in Sinks or Filters section
    for line in stdout.lines() {
        if line.contains(INS_COMBINED_SINK_PREFIX) {
            // Parse the ID from lines like:
            // │     85. ins_combined_output [vol: 1.00]
            // │ *   85. ins_combined_output [vol: 1.00]
            // │  *   47. ins_combined_my_sink                                         [Audio/Sink]
            let cleaned = line
                .replace(['│', '├', '└', '─', '*'], "")
                .trim()
                .to_string();

            if let Some(dot_pos) = cleaned.find(". ") {
                let id = cleaned[..dot_pos].trim().to_string();
                if !id.is_empty() {
                    return Ok(id);
                }
            }
        }
    }

    bail!("Combined sink not found in wpctl status")
}

/// Set the combined sink as the default output
pub(super) fn set_combined_sink_as_default(ctx: &SettingsContext) -> Result<()> {
    let sink_id = find_combined_sink_id()?;

    let output = Command::new("wpctl")
        .args(["set-default", &sink_id])
        .output()
        .context("Failed to run wpctl set-default")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to set default sink: {}", stderr);
    }

    ctx.emit_success(
        "audio.combined_sink.set_default",
        "Combined sink is now the default output.",
    );

    Ok(())
}

/// Check if combined sink is currently the default output
/// The combined sink may appear in Sinks or Filters section depending on PipeWire version
pub(super) fn is_combined_sink_default() -> Result<bool> {
    let output = Command::new("wpctl")
        .arg("status")
        .output()
        .context("Failed to run wpctl status")?;

    if !output.status.success() {
        bail!("wpctl status failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Search all lines for our prefix marked with * (indicating default)
    // It can appear in Sinks or Filters section
    for line in stdout.lines() {
        // Check if this is the default (marked with *) and has our prefix
        if line.contains('*') && line.contains(INS_COMBINED_SINK_PREFIX) {
            return Ok(true);
        }
    }

    Ok(false)
}
