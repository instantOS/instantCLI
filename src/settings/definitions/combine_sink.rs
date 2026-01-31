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

/// Disable the combined sink by removing the config file and clearing all settings
/// Returns true if a restart is needed (config file existed and was removed)
fn disable_combined_sink(ctx: &mut SettingsContext) -> Result<bool> {
    let config_path = combine_sink_config_file()?;

    // Only restart if there was actually a config to remove
    let needs_restart = config_path.exists();

    if needs_restart {
        fs::remove_file(&config_path)
            .with_context(|| format!("Failed to remove {:?}", config_path))?;
    }

    // Clear all stored configuration
    ctx.set_optional_string(COMBINED_SINK_KEY, None::<String>);
    ctx.set_optional_string(COMBINED_SINK_NAME_KEY, None::<String>);

    if needs_restart {
        ctx.notify(
            "Combined Audio Sink",
            "Combined sink disabled and configuration cleared.",
        );
    } else {
        ctx.notify("Combined Audio Sink", "Combined sink was already disabled.");
    }

    Ok(needs_restart)
}

/// Enable and configure the combined sink
/// Returns true if a restart is needed (config changed), false otherwise
fn enable_combined_sink(
    ctx: &mut SettingsContext,
    selected_node_names: &[String],
    sink_name: &str,
) -> Result<bool> {
    if selected_node_names.len() < 2 {
        bail!("Select at least 2 devices to combine");
    }

    // Check if anything actually changed
    let needs_restart = config_changed(selected_node_names, sink_name)?;

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

    if needs_restart {
        ctx.notify(
            "Combined Audio Sink",
            &format!(
                "Combined sink '{}' configured with {} devices. Restart required to activate.",
                sink_name,
                selected_node_names.len()
            ),
        );
    } else {
        ctx.notify(
            "Combined Audio Sink",
            &format!(
                "Combined sink '{}' already active with these settings.",
                sink_name
            ),
        );
    }

    Ok(needs_restart)
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

/// Get the current combined sink name, or the default if none is set
fn get_current_sink_name(ctx: &SettingsContext) -> String {
    ctx.optional_string(COMBINED_SINK_NAME_KEY)
        .unwrap_or_else(|| DEFAULT_COMBINED_SINK_NAME.to_string())
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
/// Returns true if a restart is needed (config changed)
fn config_changed(desired_devices: &[String], desired_name: &str) -> Result<bool> {
    let current = match read_current_config()? {
        Some(content) => content,
        None => return Ok(true), // No config exists, so we need to create it
    };

    // Check if name changed
    let name_changed = !current.contains(&format!(r#"node.description = "{}""#, desired_name));

    // Check if device list changed - count matches in config
    let current_device_count = current.matches("node.name = \"").count();
    let devices_changed = current_device_count != desired_devices.len();

    Ok(name_changed || devices_changed)
}

/// Menu action types with their display and preview information
#[derive(Clone)]
enum MenuAction {
    Disable,
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
        ctx: &SettingsContext,
        currently_enabled: bool,
        is_default: bool,
        device_count: usize,
    ) -> FzfPreview {
        let name = get_current_sink_name(ctx);

        match self.action {
            MenuAction::Disable => PreviewBuilder::new()
                .header(NerdFont::VolumeUp, "Disable Combined Sink")
                .text("Remove the combined sink completely.")
                .blank()
                .line(colors::RED, Some(NerdFont::Warning), "This will:")
                .text("  - Remove the PipeWire config file")
                .text("  - Clear all device selections")
                .text("  - Remove the sink from your system")
                .blank()
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Info),
                    "Requires PipeWire restart",
                )
                .build(),
            MenuAction::ChangeDevices => {
                let status = if currently_enabled {
                    format!("Currently: {} ({} devices)", name, device_count)
                } else {
                    "Not currently enabled".to_string()
                };
                PreviewBuilder::new()
                    .header(NerdFont::Settings, "Change Devices")
                    .text("Select which audio outputs to include in the combined sink.")
                    .blank()
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
            MenuAction::Disable => "disable",
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
fn rename_combined_sink(ctx: &mut SettingsContext) -> Result<bool> {
    let current_name = get_current_sink_name(ctx);

    let result = prompt_text_edit(
        crate::menu_utils::TextEditPrompt::new("Combined sink name", None)
            .header("Rename your combined audio sink")
            .ghost(&current_name),
    )?;

    let new_name = match result {
        TextEditOutcome::Updated(Some(name)) => name,
        TextEditOutcome::Updated(None) => DEFAULT_COMBINED_SINK_NAME.to_string(),
        TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => return Ok(false),
    };

    // Don't update if name is the same
    if new_name == current_name {
        return Ok(false);
    }

    // Update the stored name
    ctx.set_optional_string(COMBINED_SINK_NAME_KEY, Some(new_name.clone()));

    // If combined sink is enabled, we need to regenerate the config
    let needs_restart = if is_combined_sink_enabled() {
        let stored = parse_stored_config(ctx);
        if stored.len() >= 2 {
            let node_names: Vec<String> = stored.into_iter().collect();
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

        // Track if any changes require a restart
        let mut restart_needed = false;

        // Main configuration loop
        loop {
            // Get current state from actual sink status
            let currently_enabled = is_combined_sink_enabled();
            let is_default = is_combined_sink_default().unwrap_or(false);
            let current_name = get_current_sink_name(ctx);

            // Get stored device count (only meaningful when enabled)
            let stored_config = parse_stored_config(ctx);
            let device_count = stored_config.len();

            // Build menu items
            let mut items: Vec<MenuItem> = Vec::new();

            if currently_enabled {
                items.push(MenuItem::new(
                    MenuAction::Disable,
                    "Disable combined sink",
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
                    let preview = item.preview(ctx, currently_enabled, is_default, device_count);
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
                    MenuAction::Disable => match disable_combined_sink(ctx) {
                        Ok(needs_restart) => {
                            restart_needed = needs_restart;
                            break;
                        }
                        Err(e) => {
                            ctx.emit_failure(
                                "audio.combined_sink.disable_failed",
                                &format!("Failed to disable: {}", e),
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
                                    get_current_sink_name(ctx)
                                } else {
                                    match prompt_text_edit(
                                        crate::menu_utils::TextEditPrompt::new("Sink name", None)
                                            .header("Name your combined audio sink")
                                            .ghost(DEFAULT_COMBINED_SINK_NAME),
                                    )? {
                                        TextEditOutcome::Updated(Some(n)) => n,
                                        TextEditOutcome::Updated(None) => {
                                            DEFAULT_COMBINED_SINK_NAME.to_string()
                                        }
                                        TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => {
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

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
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
                return Some(enable_combined_sink(ctx, &node_names, &name).map(|_| ()));
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
