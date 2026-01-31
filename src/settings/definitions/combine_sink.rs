//! Combined Audio Sink setting
//!
//! Allows users to create a virtual sink that combines multiple physical audio outputs,
//! enabling simultaneous playback to multiple devices (e.g., speakers + headphones).
//! Uses PipeWire's libpipewire-module-combine-stream.

use crate::common::systemd::SystemdManager;
use crate::menu_utils::{
    ChecklistResult, FzfResult, FzfSelectable, FzfWrapper, Header, TextEditOutcome,
    prompt_text_edit,
};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// PipeWire config file path
const PIPEWIRE_CONFIG_DIR: &str = "pipewire/pipewire.conf.d";
const COMBINE_SINK_CONFIG_FILE: &str = "combine-sink.conf";

/// Prefix used to identify combined sinks created by ins
/// The full node.name will be `INS_COMBINED_SINK_PREFIX` followed by a sanitized display name
const INS_COMBINED_SINK_PREFIX: &str = "ins_combined_";

/// Default display name for the combined sink
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

/// Generate a sanitized node name from a display name
/// Converts "My Combined Output" -> "ins_combined_my_combined_output"
fn display_name_to_node_name(display_name: &str) -> String {
    let sanitized = display_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    // Collapse multiple underscores
    let collapsed = sanitized
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    format!("{}{}", INS_COMBINED_SINK_PREFIX, collapsed)
}

/// Parse config file to extract device node names and display name
fn parse_config_file() -> Result<Option<(Vec<String>, String)>> {
    let content = match read_current_config()? {
        Some(c) => c,
        None => return Ok(None),
    };

    // Extract device node names from `node.name = "..."` inside stream.rules matches
    let mut devices = Vec::new();
    let mut in_matches = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("matches = [") {
            in_matches = true;
            continue;
        }
        if in_matches && trimmed.starts_with(']') {
            in_matches = false;
            continue;
        }
        if in_matches {
            // Look for: { node.name = "device_name" }
            if let Some(start) = trimmed.find("node.name = \"") {
                let after = &trimmed[start + 13..]; // after `node.name = "`
                if let Some(end) = after.find('"') {
                    devices.push(after[..end].to_string());
                }
            }
        }
    }

    // Extract display name from node.description
    let display_name = content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if let Some(start) = trimmed.strip_prefix("node.description = \"") {
                start.strip_suffix('"').map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| DEFAULT_COMBINED_SINK_NAME.to_string());

    if devices.is_empty() {
        Ok(None)
    } else {
        Ok(Some((devices, display_name)))
    }
}

/// Get the current combined sink configuration from the config file
fn get_current_config() -> (HashSet<String>, String) {
    match parse_config_file() {
        Ok(Some((devices, name))) => (devices.into_iter().collect(), name),
        _ => (HashSet::new(), DEFAULT_COMBINED_SINK_NAME.to_string()),
    }
}

/// Remove the combined sink by deleting the config file
/// Returns true if a restart is needed (config file existed and was removed)
fn remove_combined_sink(ctx: &SettingsContext) -> Result<bool> {
    let config_path = combine_sink_config_file()?;

    // Only restart if there was actually a config to remove
    let needs_restart = config_path.exists();

    if needs_restart {
        fs::remove_file(&config_path)
            .with_context(|| format!("Failed to remove {:?}", config_path))?;
        ctx.notify("Combined Audio Sink", "Combined sink removed.");
    } else {
        ctx.notify("Combined Audio Sink", "Combined sink was not configured.");
    }

    Ok(needs_restart)
}

/// Enable and configure the combined sink
/// Returns true if a restart is needed (config changed), false otherwise
fn enable_combined_sink(
    ctx: &SettingsContext,
    selected_node_names: &[String],
    display_name: &str,
) -> Result<bool> {
    if selected_node_names.len() < 2 {
        bail!("Select at least 2 devices to combine");
    }

    // Check if anything actually changed
    let needs_restart = config_changed(selected_node_names, display_name)?;

    // Skip writing if nothing changed
    if !needs_restart {
        return Ok(false);
    }

    // Generate the node.name with our prefix for detection
    let node_name = display_name_to_node_name(display_name);

    // Build the matches array for the config
    let matches: Vec<String> = selected_node_names
        .iter()
        .map(|name| format!("                    {{ node.name = \"{}\" }}", name))
        .collect();

    // Generate the PipeWire config with prefixed node.name for detection
    let config = format!(
        r#"context.modules = [
{{   name = libpipewire-module-combine-stream
    args = {{
        combine.mode = sink
        node.name = "{}"
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
        node_name,
        display_name,
        matches.join("\n")
    );

    // Ensure the config directory exists
    let config_dir = pipewire_config_path()?;
    fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create directory {:?}", config_dir))?;

    // Write the config file
    let config_path = config_dir.join(COMBINE_SINK_CONFIG_FILE);
    fs::write(&config_path, config)
        .with_context(|| format!("Failed to write config to {:?}", config_path))?;

    ctx.notify(
        "Combined Audio Sink",
        &format!(
            "Combined sink '{}' configured with {} devices. Restart required to activate.",
            display_name,
            selected_node_names.len()
        ),
    );

    Ok(true)
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

/// Offer to restart PipeWire services after configuration change
fn offer_restart(ctx: &SettingsContext) -> Result<()> {
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

/// Get the current combined sink name from the config file, or the default if none is set
fn get_current_sink_name() -> String {
    get_current_config().1
}

/// Read the current config file content if it exists
fn read_current_config() -> Result<Option<String>> {
    let config_path = combine_sink_config_file()?;
    if !config_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {:?}", config_path))?;
    Ok(Some(content))
}

/// Check if the desired config differs from current config
/// Returns true if config changed (restart needed), false if already in desired state
fn config_changed(desired_devices: &[String], desired_name: &str) -> Result<bool> {
    let (current_devices, current_name) = get_current_config();

    // If no config exists, we need to create it
    if current_devices.is_empty() && !is_combined_sink_enabled() {
        return Ok(true);
    }

    // Check if name changed
    if current_name != desired_name {
        return Ok(true);
    }

    // Check if device list changed (compare sets, order doesn't matter)
    let desired_set: HashSet<&String> = desired_devices.iter().collect();
    let current_set: HashSet<&String> = current_devices.iter().collect();

    Ok(desired_set != current_set)
}

/// Menu action types with their display and preview information
#[derive(Clone)]
enum MenuAction {
    Remove,
    ChangeDevices,
    Rename,
    SetAsDefault,
    Enable,
    Back,
}

#[derive(Clone)]
struct MenuItem {
    action: MenuAction,
    label: String,
    icon: String,
}

impl MenuItem {
    fn new(action: MenuAction, label: impl Into<String>, icon: impl Into<String>) -> Self {
        Self {
            action,
            label: label.into(),
            icon: icon.into(),
        }
    }

    fn display_text(&self) -> String {
        format!("{} {}", self.icon, self.label)
    }

    fn preview(
        &self,
        currently_enabled: bool,
        is_default: bool,
        devices: &[String],
        config_path: &str,
    ) -> FzfPreview {
        let name = get_current_sink_name();
        let device_count = devices.len();

        match self.action {
            MenuAction::Remove => {
                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::VolumeUp, "Remove Combined Sink")
                    .text("Remove the combined sink completely.")
                    .blank();

                // Show current devices
                if !devices.is_empty() {
                    builder =
                        builder.line(colors::TEAL, Some(NerdFont::VolumeUp), "Current devices:");
                    for device in devices {
                        builder = builder.text(&format!("  • {}", device));
                    }
                    builder = builder.blank();
                }

                builder = builder
                    .field("Config file", config_path)
                    .blank()
                    .line(colors::RED, Some(NerdFont::Warning), "This will:")
                    .text("  - Remove the PipeWire config file")
                    .text("  - Remove the sink from your system")
                    .blank()
                    .line(
                        colors::YELLOW,
                        Some(NerdFont::Info),
                        "Requires PipeWire restart",
                    );
                builder.build()
            }
            MenuAction::ChangeDevices => {
                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::Settings, "Change Devices")
                    .text("Select which audio outputs to include in the combined sink.")
                    .blank();

                if currently_enabled && !devices.is_empty() {
                    builder =
                        builder.line(colors::TEAL, Some(NerdFont::VolumeUp), "Current devices:");
                    for device in devices {
                        builder = builder.text(&format!("  • {}", device));
                    }
                    builder = builder.blank();
                }

                let status = if currently_enabled {
                    format!("{} ({} devices)", name, device_count)
                } else {
                    "Not currently enabled".to_string()
                };
                builder
                    .field("Status", &status)
                    .blank()
                    .line(
                        colors::SKY,
                        Some(NerdFont::Info),
                        "Requires at least 2 devices",
                    )
                    .text("Selected devices will receive audio simultaneously")
                    .build()
            }
            MenuAction::Rename => PreviewBuilder::new()
                .header(NerdFont::Edit, "Rename Combined Sink")
                .text("Change the display name shown in audio settings.")
                .blank()
                .field("Current name", &name)
                .blank()
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Warning),
                    "Will restart PipeWire to apply name change",
                )
                .build(),
            MenuAction::SetAsDefault => {
                let status = if is_default {
                    "Already set as default"
                } else if currently_enabled {
                    "Not currently default"
                } else {
                    "Sink must be enabled first"
                };
                PreviewBuilder::new()
                    .header(NerdFont::Star, "Set as Default Output")
                    .text("Make the combined sink your primary audio output.")
                    .blank()
                    .field("Status", status)
                    .blank()
                    .line(
                        colors::GREEN,
                        Some(NerdFont::Check),
                        "No restart required - takes effect immediately",
                    )
                    .build()
            }
            MenuAction::Enable => PreviewBuilder::new()
                .header(NerdFont::Plus, "Enable Combined Sink")
                .text("Create a new combined sink by selecting audio devices.")
                .blank()
                .line(
                    colors::SKY,
                    Some(NerdFont::Info),
                    "Requires at least 2 devices",
                )
                .blank()
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Warning),
                    "Will restart PipeWire to create the sink",
                )
                .build(),
            MenuAction::Back => PreviewBuilder::new()
                .header(NerdFont::ChevronLeft, "Back")
                .text("Return to the previous menu.")
                .build(),
        }
    }
}

/// Wrapper that holds both the menu item and its computed preview
#[derive(Clone)]
struct MenuItemWithPreview {
    item: MenuItem,
    preview: FzfPreview,
}

impl MenuItemWithPreview {
    fn new(item: MenuItem, preview: FzfPreview) -> Self {
        Self { item, preview }
    }
}

impl FzfSelectable for MenuItemWithPreview {
    fn fzf_display_text(&self) -> String {
        self.item.display_text()
    }

    fn fzf_key(&self) -> String {
        match self.item.action {
            MenuAction::Remove => "remove",
            MenuAction::ChangeDevices => "change_devices",
            MenuAction::Rename => "rename",
            MenuAction::SetAsDefault => "set_default",
            MenuAction::Enable => "enable",
            MenuAction::Back => "back",
        }
        .to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

/// Rename the combined sink
/// Returns true if a restart is needed (sink was enabled and name changed)
fn rename_combined_sink(ctx: &SettingsContext) -> Result<bool> {
    let current_name = get_current_sink_name();

    let result = prompt_text_edit(
        crate::menu_utils::TextEditPrompt::new("Combined sink name", None)
            .header("Rename your combined audio sink")
            .ghost(&current_name),
    )?;

    let new_name = match result {
        TextEditOutcome::Updated(Some(name)) => name,
        // Empty input with ghost text showing current name = keep current name
        TextEditOutcome::Updated(None) => current_name.clone(),
        TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => return Ok(false),
    };

    // Don't update if name is the same
    if new_name == current_name {
        return Ok(false);
    }

    // If combined sink is enabled, we need to regenerate the config with the new name
    let needs_restart = if is_combined_sink_enabled() {
        let (stored_devices, _) = get_current_config();
        if stored_devices.len() >= 2 {
            let node_names: Vec<String> = stored_devices.into_iter().collect();
            // This will write the new config and return if restart is needed
            enable_combined_sink(ctx, &node_names, &new_name)?
        } else {
            false
        }
    } else {
        false
    };

    ctx.notify("Combined Audio Sink", &format!("Renamed to '{}'", new_name));

    Ok(needs_restart)
}

pub struct CombinedAudioSink;

impl Setting for CombinedAudioSink {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("audio.combined_sink")
            .title("Combined Audio Sink")
            .icon(NerdFont::VolumeUp)
            .summary("Combine multiple audio outputs into a single virtual sink.\n\nPlay audio through multiple devices simultaneously (e.g., speakers + headphones). Select which devices to include, rename the sink, or set it as your default output. PipeWire will only be restarted when changes require it.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn get_display_state(&self, _ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        let enabled = is_combined_sink_enabled();
        let (devices, name) = get_current_config();

        let label = if enabled {
            format!("{} ({} devices)", name, devices.len())
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

        // Track if any changes require a restart
        let mut restart_needed = false;

        // Get config path for display
        let config_path_display = combine_sink_config_file()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "~/.config/pipewire/pipewire.conf.d/combine-sink.conf".to_string());

        // Main configuration loop
        loop {
            // Get current state from actual sink status
            let currently_enabled = is_combined_sink_enabled();
            let is_default = is_combined_sink_default().unwrap_or(false);
            let (stored_config, current_name) = get_current_config();
            let device_list: Vec<String> = stored_config.iter().cloned().collect();
            let device_count = device_list.len();

            // Build menu items
            let mut items: Vec<MenuItem> = Vec::new();

            if currently_enabled {
                items.push(MenuItem::new(
                    MenuAction::Remove,
                    "Remove combined sink",
                    format_icon_colored(NerdFont::Cross, colors::RED),
                ));
                items.push(MenuItem::new(
                    MenuAction::ChangeDevices,
                    format!("Change devices ({} selected)", device_count),
                    format_icon_colored(NerdFont::Settings, colors::YELLOW),
                ));
                items.push(MenuItem::new(
                    MenuAction::Rename,
                    format!("Rename: {}", current_name),
                    format_icon_colored(NerdFont::Edit, colors::BLUE),
                ));
                if !is_default {
                    items.push(MenuItem::new(
                        MenuAction::SetAsDefault,
                        "Set as default output",
                        format_icon_colored(NerdFont::Star, colors::GREEN),
                    ));
                }
            } else {
                items.push(MenuItem::new(
                    MenuAction::Enable,
                    "Enable combined sink",
                    format_icon_colored(NerdFont::Plus, colors::GREEN),
                ));
            }

            items.push(MenuItem::new(
                MenuAction::Back,
                "Back",
                format_icon_colored(NerdFont::ChevronLeft, colors::OVERLAY1),
            ));

            // Build custom previews for each item - show actual sink state
            let header_text = if currently_enabled {
                format!(
                    "Combined Audio Sink: {} (active)\n{} devices",
                    current_name, device_count
                )
            } else {
                "Combined Audio Sink: Not active".to_string()
            };

            // Build items with computed previews
            let items_with_preview: Vec<MenuItemWithPreview> = items
                .into_iter()
                .map(|item| {
                    let preview = item.preview(
                        currently_enabled,
                        is_default,
                        &device_list,
                        &config_path_display,
                    );
                    MenuItemWithPreview::new(item, preview)
                })
                .collect();

            // Use FzfWrapper to show menu with previews
            let result = FzfWrapper::builder()
                .prompt("Select action")
                .header(Header::default(&header_text))
                .select_padded(items_with_preview)?;

            match result {
                FzfResult::Selected(wrapper) => match wrapper.item.action {
                    MenuAction::Remove => match remove_combined_sink(ctx) {
                        Ok(needs_restart) => {
                            restart_needed = needs_restart;
                            break;
                        }
                        Err(e) => {
                            ctx.emit_failure(
                                "audio.combined_sink.remove_failed",
                                &format!("Failed to remove: {}", e),
                            );
                        }
                    },
                    MenuAction::SetAsDefault => {
                        if let Err(e) = set_combined_sink_as_default(ctx) {
                            ctx.emit_failure(
                                "audio.combined_sink.set_default_failed",
                                &format!("Failed to set as default: {}", e),
                            );
                        }
                        continue;
                    }
                    MenuAction::Enable | MenuAction::ChangeDevices => {
                        // Get list of available sinks
                        let sinks = match list_sinks() {
                            Ok(s) => s,
                            Err(e) => {
                                ctx.show_message(&format!("Failed to list audio sinks: {}", e));
                                continue;
                            }
                        };

                        let is_changing = matches!(wrapper.item.action, MenuAction::ChangeDevices);
                        let initial_selection: HashSet<String> = if is_changing {
                            stored_config.clone()
                        } else {
                            HashSet::new()
                        };

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
                                    ctx.show_message("Please select at least 2 devices to combine");
                                    continue;
                                }

                                let selected_names: Vec<String> = selected
                                    .iter()
                                    .map(|item| item.sink.node_name.clone())
                                    .collect();

                                // For new sinks, prompt for name
                                let name = if is_changing {
                                    get_current_sink_name()
                                } else {
                                    match prompt_text_edit(
                                        crate::menu_utils::TextEditPrompt::new("Sink name", None)
                                            .header("Name your combined audio sink")
                                            .ghost(DEFAULT_COMBINED_SINK_NAME),
                                    )? {
                                        TextEditOutcome::Updated(Some(n)) => n,
                                        // Empty input with ghost text = use the default name
                                        TextEditOutcome::Updated(None)
                                        | TextEditOutcome::Unchanged => {
                                            DEFAULT_COMBINED_SINK_NAME.to_string()
                                        }
                                        TextEditOutcome::Cancelled => {
                                            continue;
                                        }
                                    }
                                };

                                match enable_combined_sink(ctx, &selected_names, &name) {
                                    Ok(needs_restart) => {
                                        restart_needed = needs_restart;
                                    }
                                    Err(e) => {
                                        ctx.emit_failure(
                                            "audio.combined_sink.enable_failed",
                                            &format!("Failed to enable: {}", e),
                                        );
                                    }
                                }
                                break;
                            }
                            ChecklistResult::Cancelled => continue,
                            ChecklistResult::Action(_) => {}
                        }
                    }
                    MenuAction::Rename => {
                        match rename_combined_sink(ctx) {
                            Ok(needs_restart) => {
                                restart_needed = needs_restart;
                            }
                            Err(e) => {
                                ctx.emit_failure(
                                    "audio.combined_sink.rename_failed",
                                    &format!("Failed to rename: {}", e),
                                );
                            }
                        }
                        continue;
                    }
                    MenuAction::Back => break,
                },
                FzfResult::Cancelled | FzfResult::Error(_) => break,
                FzfResult::MultiSelected(_) => break,
            }
        }

        // Offer restart if any changes require it
        if restart_needed {
            offer_restart(ctx)?;
        }

        Ok(())
    }

    // No restore needed - the config file is the single source of truth.
    // If the config file exists, the sink is configured. If not, it's disabled.
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

    #[test]
    fn test_display_name_to_node_name() {
        // Test basic conversion
        assert_eq!(
            display_name_to_node_name("Combined Output"),
            "ins_combined_combined_output"
        );

        // Test with special characters
        assert_eq!(
            display_name_to_node_name("My Speakers & Headphones!"),
            "ins_combined_my_speakers_headphones"
        );

        // Test with multiple spaces
        assert_eq!(
            display_name_to_node_name("Living   Room   Speakers"),
            "ins_combined_living_room_speakers"
        );

        // Test default name
        assert_eq!(
            display_name_to_node_name(DEFAULT_COMBINED_SINK_NAME),
            "ins_combined_combined_output"
        );
    }
}
