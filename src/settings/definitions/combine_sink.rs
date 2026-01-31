//! Combined Audio Sink setting
//!
//! Allows users to create a virtual sink that combines multiple physical audio outputs,
//! enabling simultaneous playback to multiple devices (e.g., speakers + headphones).
//! Uses PipeWire's libpipewire-module-combine-stream.

use crate::common::systemd::SystemdManager;
use crate::menu_utils::{
    ChecklistResult, FzfSelectable, FzfWrapper, Header, TextEditOutcome, prompt_text_edit,
};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::OptionalStringSettingKey;
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Key for storing the combined sink configuration (list of node names)
pub const COMBINED_SINK_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("audio.combined_sink_devices");

/// Key for storing the combined sink display name
pub const COMBINED_SINK_NAME_KEY: OptionalStringSettingKey =
    OptionalStringSettingKey::new("audio.combined_sink_name");

/// PipeWire config file path
const PIPEWIRE_CONFIG_DIR: &str = "pipewire/pipewire.conf.d";
const COMBINE_SINK_CONFIG_FILE: &str = "combine-sink.conf";

/// Default name for the combined sink
const DEFAULT_COMBINED_SINK_NAME: &str = "Combined Output";

/// Information about an audio sink device
#[derive(Debug, Clone)]
struct SinkInfo {
    id: String,
    name: String,
    node_name: String,
    description: String,
    volume: Option<String>,
    is_default: bool,
}

impl SinkInfo {
    fn display_label(&self) -> String {
        let default_tag = if self.is_default {
            format!(" [{}]", format_icon_colored(NerdFont::Star, colors::GREEN))
        } else {
            String::new()
        };
        format!("{}{}", self.description, default_tag)
    }
}

impl FzfSelectable for SinkInfo {
    fn fzf_display_text(&self) -> String {
        self.display_label()
    }

    fn fzf_key(&self) -> String {
        self.node_name.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::VolumeUp, &self.description)
            .line(
                colors::TEAL,
                Some(NerdFont::Hash),
                &format!("ID: {}", self.id),
            )
            .line(
                colors::TEAL,
                Some(NerdFont::Tag),
                &format!("Node: {}", self.node_name),
            );

        if let Some(vol) = &self.volume {
            builder = builder.line(
                colors::SKY,
                Some(NerdFont::VolumeUp),
                &format!("Volume: {}", vol),
            );
        }

        if self.is_default {
            builder = builder.line(
                colors::GREEN,
                Some(NerdFont::Star),
                "Currently set as default output",
            );
        }

        builder.build()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        false
    }
}

/// Wrapper for SinkInfo with initial checked state for checklist
#[derive(Debug, Clone)]
struct SinkChecklistItem {
    sink: SinkInfo,
    initially_checked: bool,
}

impl SinkChecklistItem {
    fn new(sink: SinkInfo, checked: bool) -> Self {
        Self {
            sink,
            initially_checked: checked,
        }
    }
}

impl FzfSelectable for SinkChecklistItem {
    fn fzf_display_text(&self) -> String {
        self.sink.fzf_display_text()
    }

    fn fzf_key(&self) -> String {
        self.sink.fzf_key()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.sink.fzf_preview()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        self.initially_checked
    }
}

/// Run wpctl status and parse the Sinks section
fn list_sinks() -> Result<Vec<SinkInfo>> {
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
fn parse_wpctl_status(output: &str) -> Result<Vec<SinkInfo>> {
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
    let cleaned = line
        .replace('│', "")
        .replace('├', "")
        .replace('└', "")
        .replace('─', "")
        .trim()
        .to_string();

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
        let without_star = if trimmed.starts_with("* ") {
            &trimmed[2..]
        } else {
            trimmed
        };

        if let Some(value) = without_star.strip_prefix("node.name = \"") {
            if let Some(end) = value.find('"') {
                return Ok(value[..end].to_string());
            }
        }
    }

    bail!(
        "node.name not found in wpctl inspect output for sink {}",
        sink_id
    )
}

/// Get the path to the PipeWire config directory
fn pipewire_config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("Unable to determine user config directory")?;
    Ok(config_dir.join(PIPEWIRE_CONFIG_DIR))
}

/// Get the full path to the combine-sink config file
fn combine_sink_config_file() -> Result<PathBuf> {
    Ok(pipewire_config_path()?.join(COMBINE_SINK_CONFIG_FILE))
}

/// Check if the combined sink is currently enabled (config file exists)
fn is_combined_sink_enabled() -> bool {
    combine_sink_config_file()
        .map(|path| path.exists())
        .unwrap_or(false)
}

/// Parse stored configuration into a set of node names
fn parse_stored_config(ctx: &SettingsContext) -> HashSet<String> {
    ctx.optional_string(COMBINED_SINK_KEY)
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Filter stored config to only include valid/available devices
/// Returns the filtered node names and whether any were removed
fn filter_valid_devices(
    ctx: &mut SettingsContext,
    available_sinks: &[SinkInfo],
) -> (Vec<String>, bool) {
    let stored = parse_stored_config(ctx);
    if stored.is_empty() {
        return (Vec::new(), false);
    }

    // Build set of available node names
    let available: HashSet<String> = available_sinks
        .iter()
        .map(|s| s.node_name.clone())
        .collect();

    // Filter stored devices to only include available ones
    let valid: Vec<String> = stored
        .iter()
        .filter(|name| available.contains(*name))
        .cloned()
        .collect();

    let removed = stored.len() - valid.len();
    let was_filtered = removed > 0;

    // Update stored config if devices were removed
    if was_filtered {
        if valid.is_empty() {
            ctx.set_optional_string(COMBINED_SINK_KEY, None::<String>);
        } else {
            ctx.set_optional_string(COMBINED_SINK_KEY, Some(valid.join("\n")));
        }
    }

    (valid, was_filtered)
}

/// Disable the combined sink by removing the config file
fn disable_combined_sink(ctx: &mut SettingsContext) -> Result<()> {
    let config_path = combine_sink_config_file()?;

    if config_path.exists() {
        fs::remove_file(&config_path)
            .with_context(|| format!("Failed to remove {:?}", config_path))?;
    }

    // Clear the stored configuration
    ctx.set_optional_string(COMBINED_SINK_KEY, None::<String>);
    ctx.set_optional_string(COMBINED_SINK_NAME_KEY, None::<String>);

    ctx.notify("Combined Audio Sink", "Combined sink disabled.");

    // Offer to restart PipeWire to apply changes (don't offer set default after disable)
    offer_restart(ctx, false)?;

    Ok(())
}

/// Enable and configure the combined sink
/// This overwrites the existing config file if it exists
fn enable_combined_sink(
    ctx: &mut SettingsContext,
    _available_sinks: &[SinkInfo],
    selected_node_names: &[String],
    sink_name: &str,
) -> Result<()> {
    if selected_node_names.len() < 2 {
        bail!("Select at least 2 devices to combine");
    }

    // Build the matches array for the config
    let matches: Vec<String> = selected_node_names
        .iter()
        .map(|name| format!("                    {{ node.name = \"{}\" }}", name))
        .collect();

    // Generate the PipeWire config
    let config = format!(
        r#"context.modules = [
{{   name = libpipewire-module-combine-stream
    args = {{
        combine.mode = sink
        node.name = "combined_output"
        node.description = "{}"
        combine.props = {{
            audio.position = [ FL FR ]
        }}
        stream.rules = [
            {{
                matches = [
{}
                ]
                actions = {{
                    create-stream = {{ }}
                }}
            }}
        ]
    }}
}}
]
"#,
        sink_name,
        matches.join("\n")
    );

    // Ensure the config directory exists
    let config_dir = pipewire_config_path()?;
    fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create directory {:?}", config_dir))?;

    // Write the config file (overwrites if exists)
    let config_path = config_dir.join(COMBINE_SINK_CONFIG_FILE);
    fs::write(&config_path, config)
        .with_context(|| format!("Failed to write config to {:?}", config_path))?;

    // Store the configuration
    ctx.set_optional_string(COMBINED_SINK_KEY, Some(selected_node_names.join("\n")));
    ctx.set_optional_string(COMBINED_SINK_NAME_KEY, Some(sink_name.to_string()));

    ctx.notify(
        "Combined Audio Sink",
        &format!(
            "Combined sink '{}' configured with {} devices.",
            sink_name,
            selected_node_names.len()
        ),
    );

    // Offer to restart PipeWire to apply changes (and offer set default after enable)
    offer_restart(ctx, true)?;

    Ok(())
}

/// Restart PipeWire services to apply configuration changes
fn restart_pipewire_services(ctx: &SettingsContext) -> Result<()> {
    let manager = SystemdManager::user();

    ctx.emit_info(
        "audio.combined_sink.restarting",
        "Restarting PipeWire services...",
    );

    // Restart the main PipeWire services in order
    // wireplumber should auto-restart since it depends on pipewire
    manager.restart("pipewire")?;

    ctx.emit_success(
        "audio.combined_sink.restarted",
        "PipeWire services restarted successfully.",
    );

    Ok(())
}

/// Find the ID of the combined sink from wpctl status
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

    // Search all lines for combined_output - it can appear in Sinks or Filters section
    for line in stdout.lines() {
        if line.contains("combined_output") {
            // Parse the ID from lines like:
            // │     85. combined_output [vol: 1.00]
            // │ *   85. combined_output [vol: 1.00]
            // │  *   47. combined_output                                              [Audio/Sink]
            let cleaned = line
                .replace('│', "")
                .replace('├', "")
                .replace('└', "")
                .replace('─', "")
                .replace('*', "")
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
fn set_combined_sink_as_default(ctx: &SettingsContext) -> Result<()> {
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
fn is_combined_sink_default() -> Result<bool> {
    let output = Command::new("wpctl")
        .arg("status")
        .output()
        .context("Failed to run wpctl status")?;

    if !output.status.success() {
        bail!("wpctl status failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Search all lines for combined_output marked with * (indicating default)
    // It can appear in Sinks or Filters section
    for line in stdout.lines() {
        // Check if this is the default (marked with *) and is combined_output
        if line.contains('*') && line.contains("combined_output") {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Offer to restart PipeWire services after configuration change
fn offer_restart(ctx: &SettingsContext, offer_set_default: bool) -> Result<()> {
    let result = FzfWrapper::builder()
        .confirm("PipeWire needs to be restarted for changes to take effect.\n\nAudio will be briefly interrupted during restart.")
        .yes_text("Restart PipeWire")
        .no_text("Restart Later (manual)")
        .confirm_dialog()?;

    match result {
        crate::menu_utils::ConfirmResult::Yes => {
            if let Err(e) = restart_pipewire_services(ctx) {
                ctx.emit_failure(
                    "audio.combined_sink.restart_failed",
                    &format!("Failed to restart PipeWire: {}", e),
                );
                ctx.show_message(&format!(
                    "Failed to restart PipeWire: {}\n\nPlease restart manually:\n  systemctl --user restart pipewire",
                    e
                ));
            } else if offer_set_default {
                // After successful restart, offer to set as default
                offer_set_default_output(ctx)?;
            }
        }
        crate::menu_utils::ConfirmResult::No | crate::menu_utils::ConfirmResult::Cancelled => {
            ctx.emit_info(
                "audio.combined_sink.restart_skipped",
                "PipeWire restart skipped. Run 'systemctl --user restart pipewire' to apply changes.",
            );
        }
    }

    Ok(())
}

/// Offer to set the combined sink as the default output
fn offer_set_default_output(ctx: &SettingsContext) -> Result<()> {
    // Check if combined sink is already the default
    match is_combined_sink_default() {
        Ok(true) => {
            ctx.emit_info(
                "audio.combined_sink.already_default",
                "Combined sink is already the default output.",
            );
            return Ok(());
        }
        Ok(false) => {
            // Not default, offer to set it
        }
        Err(e) => {
            // Failed to check, but we can still try to set it
            ctx.emit_info(
                "audio.combined_sink.check_default_failed",
                &format!("Could not check if combined sink is default: {}", e),
            );
        }
    }

    let result = FzfWrapper::builder()
        .confirm("The combined sink has been created.\n\nWould you like to set it as the default audio output?")
        .yes_text("Set as Default")
        .no_text("Keep Current Default")
        .confirm_dialog()?;

    match result {
        crate::menu_utils::ConfirmResult::Yes => {
            if let Err(e) = set_combined_sink_as_default(ctx) {
                ctx.emit_failure(
                    "audio.combined_sink.set_default_failed",
                    &format!("Failed to set as default: {}", e),
                );
            }
        }
        crate::menu_utils::ConfirmResult::No | crate::menu_utils::ConfirmResult::Cancelled => {
            ctx.emit_info(
                "audio.combined_sink.default_skipped",
                "You can set the combined sink as default later from the main menu.",
            );
        }
    }

    Ok(())
}

/// Get the current combined sink name, or the default if none is set
fn get_current_sink_name(ctx: &SettingsContext) -> String {
    ctx.optional_string(COMBINED_SINK_NAME_KEY)
        .unwrap_or_else(|| DEFAULT_COMBINED_SINK_NAME.to_string())
}

/// Prompt for a new combined sink name and update the configuration
fn rename_combined_sink(ctx: &mut SettingsContext) -> Result<()> {
    let current_name = get_current_sink_name(ctx);

    let result = prompt_text_edit(
        crate::menu_utils::TextEditPrompt::new("Combined sink name", None)
            .header("Rename your combined audio sink")
            .ghost(&current_name),
    )?;

    let new_name = match result {
        TextEditOutcome::Updated(Some(name)) => name,
        TextEditOutcome::Updated(None) => DEFAULT_COMBINED_SINK_NAME.to_string(),
        TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => return Ok(()),
    };

    // Update the stored name
    ctx.set_optional_string(COMBINED_SINK_NAME_KEY, Some(new_name.clone()));

    // If combined sink is enabled, regenerate the config with the new name
    if is_combined_sink_enabled() {
        let stored = parse_stored_config(ctx);
        if stored.len() >= 2 {
            let node_names: Vec<String> = stored.into_iter().collect();
            enable_combined_sink(ctx, &[], &node_names, &new_name)?;
        }
    }

    ctx.notify("Combined Audio Sink", &format!("Renamed to '{}'", new_name));

    Ok(())
}

pub struct CombinedAudioSink;

impl Setting for CombinedAudioSink {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("audio.combined_sink")
            .title("Combined Audio Sink")
            .icon(NerdFont::VolumeUp)
            .summary("Combine multiple audio outputs into a single virtual sink.\n\nPlay audio through multiple devices simultaneously (e.g., speakers + headphones). Select which devices to include, rename the sink, or set it as your default output. PipeWire will be restarted to apply changes (with your confirmation).")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn get_display_state(&self, ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        let enabled = is_combined_sink_enabled();
        let stored = ctx.optional_string(COMBINED_SINK_KEY);
        let name = ctx
            .optional_string(COMBINED_SINK_NAME_KEY)
            .unwrap_or_else(|| DEFAULT_COMBINED_SINK_NAME.to_string());

        let label = if enabled {
            let device_count = stored.map(|s| s.lines().count()).unwrap_or(0);
            format!("{} ({} devices)", name, device_count)
        } else {
            "Not configured".to_string()
        };

        crate::settings::setting::SettingState::Choice {
            current_label: label,
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        // Check if wpctl is available
        if which::which("wpctl").is_err() {
            ctx.show_message("wpctl not found. Is PipeWire installed?");
            return Ok(());
        }

        // Get list of available sinks
        let sinks = match list_sinks() {
            Ok(s) => s,
            Err(e) => {
                ctx.show_message(&format!("Failed to list audio sinks: {}", e));
                return Ok(());
            }
        };

        // Filter stored config to only include valid devices
        let (valid_devices, devices_removed) = filter_valid_devices(ctx, &sinks);
        if devices_removed {
            ctx.emit_info(
                "audio.combined_sink.devices_removed",
                "Some previously selected devices are no longer available and have been removed from the configuration.",
            );
        }

        let currently_enabled = is_combined_sink_enabled();
        let stored_config: HashSet<String> = valid_devices.iter().cloned().collect();

        // Main configuration loop
        loop {
            // Build menu items
            let mut items: Vec<(String, String)> = Vec::new();

            // Check if combined sink is the current default
            let is_default = is_combined_sink_default().unwrap_or(false);
            let current_name = get_current_sink_name(ctx);

            // Status and toggle option
            if currently_enabled {
                items.push((
                    "disable".to_string(),
                    format!(
                        "{} Disable combined sink",
                        format_icon_colored(NerdFont::Cross, colors::RED)
                    ),
                ));
                items.push((
                    "reconfigure".to_string(),
                    format!(
                        "{} Change devices ({} selected)",
                        format_icon_colored(NerdFont::Settings, colors::YELLOW),
                        stored_config.len()
                    ),
                ));
                items.push((
                    "rename".to_string(),
                    format!(
                        "{} Rename: {}",
                        format_icon_colored(NerdFont::Edit, colors::BLUE),
                        current_name
                    ),
                ));
                // Show "Set as default" option if not already default
                if !is_default {
                    items.push((
                        "set_default".to_string(),
                        format!(
                            "{} Set as default output",
                            format_icon_colored(NerdFont::Star, colors::GREEN)
                        ),
                    ));
                }
            } else if !valid_devices.is_empty() {
                // Show re-enable option if we have stored devices but config was removed
                items.push((
                    "enable".to_string(),
                    format!(
                        "{} Re-enable combined sink ({} devices)",
                        format_icon_colored(NerdFont::Plus, colors::GREEN),
                        valid_devices.len()
                    ),
                ));
                items.push((
                    "reconfigure".to_string(),
                    format!(
                        "{} Change devices",
                        format_icon_colored(NerdFont::Settings, colors::YELLOW)
                    ),
                ));
                items.push((
                    "rename".to_string(),
                    format!(
                        "{} Rename: {}",
                        format_icon_colored(NerdFont::Edit, colors::BLUE),
                        current_name
                    ),
                ));
            } else {
                items.push((
                    "enable".to_string(),
                    format!(
                        "{} Enable combined sink",
                        format_icon_colored(NerdFont::Plus, colors::GREEN)
                    ),
                ));
            }

            // Always show back option
            items.push((
                "back".to_string(),
                format!(
                    "{} Back",
                    format_icon_colored(NerdFont::ChevronLeft, colors::OVERLAY1)
                ),
            ));

            let selection = dialoguer::Select::new()
                .with_prompt("Combined Audio Sink")
                .items(
                    &items
                        .iter()
                        .map(|(_, label)| label.as_str())
                        .collect::<Vec<_>>(),
                )
                .default(0)
                .interact_opt()
                .context("Failed to show selection dialog")?;

            match selection {
                Some(idx) => {
                    let action = &items[idx].0;
                    match action.as_str() {
                        "disable" => {
                            if let Err(e) = disable_combined_sink(ctx) {
                                ctx.emit_failure(
                                    "audio.combined_sink.disable_failed",
                                    &format!("Failed to disable: {}", e),
                                );
                            }
                            break;
                        }
                        "set_default" => {
                            if let Err(e) = set_combined_sink_as_default(ctx) {
                                ctx.emit_failure(
                                    "audio.combined_sink.set_default_failed",
                                    &format!("Failed to set as default: {}", e),
                                );
                            }
                            // Continue to show the menu after setting default
                            continue;
                        }
                        "enable" | "reconfigure" => {
                            // Build initial selection set
                            let initial_selection: HashSet<String> = if action == "reconfigure" {
                                stored_config.clone()
                            } else {
                                HashSet::new()
                            };

                            // Build checklist items with initial checked state
                            let checklist_items: Vec<SinkChecklistItem> = sinks
                                .iter()
                                .map(|sink| {
                                    let checked = initial_selection.contains(&sink.node_name);
                                    SinkChecklistItem::new(sink.clone(), checked)
                                })
                                .collect();

                            let header_text = format!(
                                "Select at least 2 audio devices to combine\nSelected devices will receive audio simultaneously."
                            );
                            let header = Header::default(&header_text);

                            let result = FzfWrapper::builder()
                                .prompt("Select devices")
                                .header(header)
                                .checklist("Combine")
                                .checklist_dialog(checklist_items)?;

                            match result {
                                ChecklistResult::Confirmed(selected) => {
                                    if selected.len() < 2 {
                                        ctx.show_message(
                                            "Please select at least 2 devices to combine",
                                        );
                                        continue;
                                    }

                                    // Extract SinkInfo from SinkChecklistItem
                                    let selected_names: Vec<String> = selected
                                        .iter()
                                        .map(|item| item.sink.node_name.clone())
                                        .collect();

                                    // Use existing name or default
                                    let name = get_current_sink_name(ctx);

                                    if let Err(e) =
                                        enable_combined_sink(ctx, &sinks, &selected_names, &name)
                                    {
                                        ctx.emit_failure(
                                            "audio.combined_sink.enable_failed",
                                            &format!("Failed to enable: {}", e),
                                        );
                                    }
                                    break;
                                }
                                ChecklistResult::Cancelled => continue,
                                ChecklistResult::Action(_) => {}
                            }
                        }
                        "rename" => {
                            if let Err(e) = rename_combined_sink(ctx) {
                                ctx.emit_failure(
                                    "audio.combined_sink.rename_failed",
                                    &format!("Failed to rename: {}", e),
                                );
                            }
                            continue;
                        }
                        "back" | _ => break,
                    }
                }
                None => break,
            }
        }

        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        // Check if we need to restore the combined sink configuration
        if !is_combined_sink_enabled() {
            let stored = ctx.optional_string(COMBINED_SINK_KEY)?;
            let name = ctx
                .optional_string(COMBINED_SINK_NAME_KEY)
                .unwrap_or_else(|| DEFAULT_COMBINED_SINK_NAME.to_string());

            let node_names: Vec<String> = stored
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();

            if node_names.len() >= 2 {
                // Re-create the config file with stored settings
                // The config file is overwritten, not appended
                return Some(enable_combined_sink(ctx, &[], &node_names, &name));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wpctl_status() {
        let sample_output = r#"PipeWire 'pipewire-0' [1.4.9]
 └─ Clients:
        33. WirePlumber
Audio
 ├─ Devices:
 │      32. Radeon High Definition Audio Controller [alsa]
 │
 ├─ Sinks:
 │      48. DualSense wireless controller (PS5) 0 [vol: 1.00]
 │  *   78. Radeon High Definition Audio Controller Digitales Stereo (HDMI) [vol: 0.95]
 │
 ├─ Sources:
 │      47. Ryzen HD Audio Controller Analoges Stereo
"#;

        let sinks = parse_wpctl_status(sample_output).unwrap();
        assert_eq!(sinks.len(), 2);
        assert_eq!(sinks[0].id, "48");
        assert!(!sinks[0].is_default);
        assert_eq!(sinks[1].id, "78");
        assert!(sinks[1].is_default);
    }
}
