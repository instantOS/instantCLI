//! Combined Audio Sink setting
//!
//! Allows users to create a virtual sink that combines multiple physical audio outputs,
//! enabling simultaneous playback to multiple devices (e.g., speakers + headphones).
//! Uses PipeWire's libpipewire-module-combine-stream.

use crate::menu_utils::{
    prompt_text_edit, ChecklistResult, FzfSelectable, FzfWrapper, Header, TextEditOutcome,
};
use crate::menu_utils::{select_one_with_style_at, MenuCursor};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::OptionalStringSettingKey;
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};
use anyhow::{bail, Context, Result};
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
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("  node.name = \"") {
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

    ctx.notify(
        "Combined Audio Sink",
        "Combined sink disabled. Restart PipeWire to apply changes.",
    );

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
            "Combined sink '{}' configured with {} devices. Restart PipeWire to apply changes.",
            sink_name,
            selected_node_names.len()
        ),
    );

    Ok(())
}

/// Menu items for name selection
#[derive(Clone)]
struct NameOption {
    action: NameAction,
    label: String,
    current_name: String,
}

#[derive(Clone, Copy, PartialEq)]
enum NameAction {
    KeepCurrent,
    EnterNew,
    Back,
}

impl NameOption {
    fn new(action: NameAction, current_name: &str) -> Self {
        let label = match action {
            NameAction::KeepCurrent => {
                format!("{} Keep: {}", NerdFont::Check, current_name)
            }
            NameAction::EnterNew => format!("{} Enter new name...", NerdFont::Edit),
            NameAction::Back => format!("{} Back", format_back_icon()),
        };
        Self {
            action,
            label,
            current_name: current_name.to_string(),
        }
    }
}

impl FzfSelectable for NameOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_key(&self) -> String {
        match self.action {
            NameAction::KeepCurrent => "keep".to_string(),
            NameAction::EnterNew => "new".to_string(),
            NameAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let content = match self.action {
            NameAction::KeepCurrent => PreviewBuilder::new()
                .text("Use the current name for the combined sink.")
                .blank()
                .field("Current name", &self.current_name)
                .build(),
            NameAction::EnterNew => PreviewBuilder::new()
                .text("Enter a new custom name for your combined audio sink.")
                .blank()
                .field("Current name", &self.current_name)
                .build(),
            NameAction::Back => PreviewBuilder::new()
                .text("Go back without changing the name.")
                .build(),
        };
        content
    }
}

/// Prompt for combined sink name using menu_utils
fn prompt_for_sink_name(ctx: &SettingsContext, default: &str) -> Result<Option<(String, bool)>> {
    let current_name = ctx
        .optional_string(COMBINED_SINK_NAME_KEY)
        .unwrap_or_else(|| default.to_string());

    // Build menu items
    let items = vec![
        NameOption::new(NameAction::KeepCurrent, &current_name),
        NameOption::new(NameAction::EnterNew, &current_name),
        NameOption::new(NameAction::Back, &current_name),
    ];

    let mut cursor = MenuCursor::new();

    loop {
        let current_items = items.clone();
        let initial_cursor = cursor.initial_index(&current_items);
        let selection = select_one_with_style_at(current_items, initial_cursor)?;

        match selection {
            Some(option) => {
                cursor.update(&option, &items);

                match option.action {
                    NameAction::KeepCurrent => return Ok(Some((current_name.clone(), true))),
                    NameAction::EnterNew => {
                        // Use text input for new name
                        let result = prompt_text_edit(
                            crate::menu_utils::TextEditPrompt::new("Combined sink name", None)
                                .header("Enter a name for your combined audio sink")
                                .ghost(&current_name),
                        )?;

                        match result {
                            TextEditOutcome::Updated(Some(name)) => return Ok(Some((name, true))),
                            TextEditOutcome::Updated(None) => {
                                // User cleared the name - use default
                                return Ok(Some((DEFAULT_COMBINED_SINK_NAME.to_string(), true)));
                            }
                            TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => continue,
                        }
                    }
                    NameAction::Back => return Ok(None),
                }
            }
            None => return Ok(None),
        }
    }
}

pub struct CombinedAudioSink;

impl Setting for CombinedAudioSink {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("audio.combined_sink")
            .title("Combined Audio Sink")
            .icon(NerdFont::VolumeUp)
            .summary("Combine multiple audio outputs into a single virtual sink.\n\nPlay audio through multiple devices simultaneously (e.g., speakers + headphones). Creates a PipeWire combined sink configuration.")
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
                        "{} Reconfigure devices ({} currently selected)",
                        format_icon_colored(NerdFont::Settings, colors::YELLOW),
                        stored_config.len()
                    ),
                ));
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
                        "{} Reconfigure devices",
                        format_icon_colored(NerdFont::Settings, colors::YELLOW)
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
                        "enable" | "reconfigure" => {
                            // Show checklist of available sinks
                            // Pre-select previously configured devices
                            let available_node_names: HashSet<String> =
                                sinks.iter().map(|s| s.node_name.clone()).collect();

                            // If reconfiguring with valid stored devices, use those as initial selection
                            let initial_selection: HashSet<String> = if action == "reconfigure" {
                                stored_config
                                    .intersection(&available_node_names)
                                    .cloned()
                                    .collect()
                            } else {
                                HashSet::new()
                            };

                            // Build checklist items with proper initial state
                            let checklist_items: Vec<(SinkInfo, bool)> = sinks
                                .iter()
                                .map(|sink| {
                                    let checked = initial_selection.contains(&sink.node_name);
                                    (sink.clone(), checked)
                                })
                                .collect();

                            let header_text = format!(
                                "Select at least 2 audio devices to combine into '{}'\nSelected devices will receive audio simultaneously.",
                                ctx.optional_string(COMBINED_SINK_NAME_KEY)
                                    .unwrap_or_else(|| DEFAULT_COMBINED_SINK_NAME.to_string())
                            );
                            let header = Header::default(&header_text);

                            // Convert to SinkInfo for the checklist dialog
                            // We need to mark checked items
                            let result = FzfWrapper::builder()
                                .prompt("Select devices")
                                .header(header)
                                .checklist("Combine")
                                .checklist_dialog_with_initial(
                                    checklist_items.into_iter().map(|(item, _)| item).collect(),
                                    &initial_selection,
                                )?;

                            match result {
                                ChecklistResult::Confirmed(selected) => {
                                    if selected.len() < 2 {
                                        ctx.show_message(
                                            "Please select at least 2 devices to combine",
                                        );
                                        continue;
                                    }

                                    let selected_names: Vec<String> =
                                        selected.iter().map(|s| s.node_name.clone()).collect();

                                    // Get name for the sink
                                    match prompt_for_sink_name(ctx, DEFAULT_COMBINED_SINK_NAME) {
                                        Ok(Some((name, _))) => {
                                            if let Err(e) = enable_combined_sink(
                                                ctx,
                                                &sinks,
                                                &selected_names,
                                                &name,
                                            ) {
                                                ctx.emit_failure(
                                                    "audio.combined_sink.enable_failed",
                                                    &format!("Failed to enable: {}", e),
                                                );
                                            }
                                        }
                                        Ok(None) => continue,
                                        Err(e) => {
                                            ctx.emit_failure(
                                                "audio.combined_sink.name_error",
                                                &format!("Name input error: {}", e),
                                            );
                                        }
                                    }
                                    break;
                                }
                                ChecklistResult::Cancelled => continue,
                                ChecklistResult::Action(_) => {}
                            }
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
