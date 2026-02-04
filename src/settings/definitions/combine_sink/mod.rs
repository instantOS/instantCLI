//! Combined Audio Sink setting
//!
//! Allows users to create a virtual sink that combines multiple physical audio outputs,
//! enabling simultaneous playback to multiple devices (e.g., speakers + headphones).
//! Uses PipeWire's libpipewire-module-combine-stream.

use std::collections::HashSet;

use anyhow::Result;

use crate::menu_utils::{ChecklistResult, FzfResult, FzfWrapper, Header};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

mod actions;
mod config;
mod menu;
mod model;
mod wpctl;

use actions::{
    enable_combined_sink, offer_restart, offer_set_as_default, remove_combined_sink,
    rename_combined_sink,
};
use config::{
    combine_sink_config_file, get_current_config, get_current_sink_name, is_combined_sink_enabled,
};
use menu::{MenuAction, build_header_text, build_items_with_previews, build_menu_items};
use model::SinkChecklistItem;
use wpctl::{is_combined_sink_default, list_sinks, set_combined_sink_as_default};

/// PipeWire config file path
const PIPEWIRE_CONFIG_DIR: &str = "pipewire/pipewire.conf.d";
const COMBINE_SINK_CONFIG_FILE: &str = "combine-sink.conf";

/// Prefix used to identify combined sinks created by ins
/// The full node.name will be `INS_COMBINED_SINK_PREFIX` followed by a sanitized display name
const INS_COMBINED_SINK_PREFIX: &str = "ins_combined_";

/// Default display name for the combined sink
const DEFAULT_COMBINED_SINK_NAME: &str = "Combined Output";

pub struct CombinedAudioSink;

impl Setting for CombinedAudioSink {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("audio.combined_sink")
            .title("Combined Audio Sink")
            .icon(NerdFont::VolumeUp)
            .summary("Combine multiple audio outputs into a single virtual sink.\n\nPlay audio through multiple devices simultaneously (e.g., speakers + headphones). Select which devices to include, rename the sink, or set it as your default output. PipeWire will only be restarted when changes require it.")
            .requires_reapply(true)
            .search_keywords(&["audio", "volume", "sound", "sink", "output", "combine", "multi"])
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

            let items =
                build_menu_items(currently_enabled, is_default, device_count, &current_name);

            // Build custom previews for each item - show actual sink state
            let header_text = build_header_text(currently_enabled, &current_name, device_count);

            // Build items with computed previews
            let items_with_preview = build_items_with_previews(
                items,
                currently_enabled,
                is_default,
                &device_list,
                &config_path_display,
            );

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

                        let header_text = "Select at least 2 audio devices to combine\nSelected devices will receive audio simultaneously.".to_string();
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

                                // Use current name for changes, default name for new sinks
                                let name = if is_changing {
                                    get_current_sink_name()
                                } else {
                                    DEFAULT_COMBINED_SINK_NAME.to_string()
                                };

                                match enable_combined_sink(ctx, &selected_names, &name) {
                                    Ok(needs_restart) => {
                                        restart_needed = needs_restart;
                                        // For new sinks, offer to set as default
                                        if !is_changing && needs_restart {
                                            // Continue to menu so user can see "Set as default" option
                                            // after restart is offered
                                        }
                                    }
                                    Err(e) => {
                                        ctx.emit_failure(
                                            "audio.combined_sink.enable_failed",
                                            &format!("Failed to enable: {}", e),
                                        );
                                    }
                                }
                                // After enabling/changing, break to offer restart,
                                // then we'll offer to set as default
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

            // After enabling a new combined sink and restarting, offer to set as default
            if is_combined_sink_enabled() && !is_combined_sink_default().unwrap_or(false) {
                offer_set_as_default(ctx)?;
            }
        }

        Ok(())
    }

    // No restore needed - the config file is the single source of truth.
    // If the config file exists, the sink is configured. If not, it's disabled.
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_COMBINED_SINK_NAME;
    use super::config::display_name_to_node_name;
    use super::wpctl::parse_wpctl_status;

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
