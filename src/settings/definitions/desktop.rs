//! Desktop settings (additional)
//!
//! Window layout and other desktop settings.

use anyhow::Result;
use std::process::Command;

use crate::common::audio::{
    default_source_names, list_audio_sources_short, pactl_defaults, AudioDefaults, AudioSourceInfo,
};
use crate::common::compositor::CompositorType;
use crate::menu_utils::{
    select_one_with_style_at, ChecklistAction, ChecklistResult, FzfSelectable, FzfWrapper, Header,
    MenuCursor,
};
use crate::settings::context::SettingsContext;
use crate::settings::deps::{BLUEMAN, PIPER};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{
    is_audio_sources_default, parse_audio_source_selection, OptionalStringSettingKey,
    StringSettingKey, SCREEN_RECORD_AUDIO_SOURCES_DEFAULT, SCREEN_RECORD_AUDIO_SOURCES_KEY,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

// ============================================================================
// Window Layout (interactive selection, can't use macro)
// ============================================================================

pub struct WindowLayout;

impl WindowLayout {
    const KEY: StringSettingKey = StringSettingKey::new("desktop.layout", "tile");
}

#[derive(Clone)]
struct LayoutChoice {
    value: &'static str,
    label: &'static str,
    description: &'static str,
}

#[derive(Clone)]
struct LayoutChoiceDisplay {
    choice: Option<&'static LayoutChoice>,
    is_current: bool,
}

impl FzfSelectable for LayoutChoiceDisplay {
    fn fzf_display_text(&self) -> String {
        match self.choice {
            Some(choice) => {
                let icon = if self.is_current {
                    format_icon_colored(NerdFont::CheckSquare, colors::GREEN)
                } else {
                    format_icon_colored(NerdFont::Square, colors::OVERLAY1)
                };
                format!("{} {}", icon, choice.label)
            }
            None => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self.choice {
            Some(choice) => crate::menu_utils::FzfPreview::Text(choice.description.to_string()),
            None => crate::menu_utils::FzfPreview::Text("Go back to the previous menu".to_string()),
        }
    }

    fn fzf_key(&self) -> String {
        match self.choice {
            Some(choice) => choice.value.to_string(),
            None => "__back__".to_string(),
        }
    }
}

/// Apply a window layout via instantwmctl
fn apply_window_layout(ctx: &mut SettingsContext, layout: &str) -> Result<()> {
    let compositor = CompositorType::detect();
    if !matches!(compositor, CompositorType::InstantWM) {
        ctx.emit_unsupported(
            "settings.desktop.layout.unsupported",
            &format!(
                "Window layout configuration is only supported on instantwm. Detected: {}. Setting saved but not applied.",
                compositor.name()
            ),
        );
        return Ok(());
    }

    let status = Command::new("instantwmctl")
        .args(["layout", layout])
        .status();

    match status {
        Ok(exit) if exit.success() => {
            ctx.notify("Window Layout", &format!("Set to: {layout}"));
        }
        Ok(exit) => {
            ctx.emit_failure(
                "settings.desktop.layout.apply_failed",
                &format!(
                    "Failed to apply layout '{layout}' (exit code {}).",
                    exit.code().unwrap_or(-1)
                ),
            );
        }
        Err(err) => {
            ctx.emit_failure(
                "settings.desktop.layout.apply_error",
                &format!("Failed to run instantwmctl: {err}"),
            );
        }
    }

    Ok(())
}

/// Build the display items list with current selection marked
fn build_layout_items(current: &str) -> Vec<LayoutChoiceDisplay> {
    let mut items: Vec<LayoutChoiceDisplay> = LAYOUT_OPTIONS
        .iter()
        .map(|choice| LayoutChoiceDisplay {
            choice: Some(choice),
            is_current: choice.value == current,
        })
        .collect();

    // Add Back entry at bottom
    items.push(LayoutChoiceDisplay {
        choice: None,
        is_current: false,
    });

    items
}

const LAYOUT_OPTIONS: &[LayoutChoice] = &[
    LayoutChoice {
        value: "tile",
        label: "Tile",
        description: "Windows split the screen side-by-side (recommended for most users)",
    },
    LayoutChoice {
        value: "grid",
        label: "Grid",
        description: "Windows arranged in an even grid pattern",
    },
    LayoutChoice {
        value: "float",
        label: "Float",
        description: "Windows can be freely moved and resized (like Windows/macOS)",
    },
    LayoutChoice {
        value: "monocle",
        label: "Monocle",
        description: "One window fills the entire screen at a time",
    },
    LayoutChoice {
        value: "tcl",
        label: "Three Columns",
        description: "Main window in center, others on sides",
    },
    LayoutChoice {
        value: "deck",
        label: "Deck",
        description: "Large main window with smaller windows stacked on the side",
    },
    LayoutChoice {
        value: "overviewlayout",
        label: "Overview",
        description: "See all your workspaces at once",
    },
    LayoutChoice {
        value: "bstack",
        label: "Bottom Stack",
        description: "Main window on top, others stacked below",
    },
    LayoutChoice {
        value: "bstackhoriz",
        label: "Bottom Stack (Horizontal)",
        description: "Main window on top, others arranged horizontally below",
    },
];

impl Setting for WindowLayout {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("desktop.layout")
            .title("Window Layout")
            .icon(NerdFont::List)
            .summary("Choose how windows are arranged on your screen by default.\n\nYou can always change the layout temporarily with keyboard shortcuts.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Choice { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.string(Self::KEY);
        let initial_index = LAYOUT_OPTIONS
            .iter()
            .position(|l| l.value == current)
            .unwrap_or(0);

        let mut cursor = MenuCursor::new();

        loop {
            let items = build_layout_items(&ctx.string(Self::KEY));
            let initial_cursor = cursor.initial_index(&items).or(Some(initial_index));
            let selection = select_one_with_style_at(items.clone(), initial_cursor)?;

            match selection {
                Some(display) => {
                    cursor.update(&display, &items);

                    match display.choice {
                        Some(choice) => {
                            ctx.set_string(Self::KEY, choice.value);
                            apply_window_layout(ctx, choice.value)?;
                        }
                        None => break, // Back selected
                    }
                }
                None => break,
            }
        }

        Ok(())
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::InstantWM) {
            return None;
        }

        let layout = ctx.string(Self::KEY);
        Some(apply_window_layout(ctx, &layout))
    }
}

// ============================================================================
// Gaming Mouse (GUI app)
// ============================================================================

gui_command_setting!(
    GamingMouse,
    "mouse.gaming",
    "Gaming Mouse Customization",
    NerdFont::Mouse,
    "Configure gaming mice with customizable buttons, RGB lighting, and DPI settings.\n\nUses Piper to configure Logitech and other gaming mice supported by libratbag.",
    "piper",
    &PIPER
);

// ============================================================================
// Bluetooth Manager (GUI app)
// ============================================================================

gui_command_setting!(
    BluetoothManager,
    "bluetooth.manager",
    "Manage Devices",
    NerdFont::Settings,
    "Pair new devices and manage connected Bluetooth devices.\n\nUse this to connect headphones, speakers, keyboards, mice, and other wireless devices.",
    "blueman-manager",
    &BLUEMAN
);

// ============================================================================
// Screen Recording Audio Sources
// ============================================================================

#[derive(Clone)]
struct AudioSourceItem {
    name: String,
    label: String,
    checked: bool,
    is_default_input: bool,
    is_default_output: bool,
    is_monitor: bool,
    driver: Option<String>,
    sample_spec: Option<String>,
    channel_map: Option<String>,
    state: Option<String>,
}

impl AudioSourceItem {
    fn new(info: AudioSourceInfo, checked: bool, defaults: &AudioDefaults) -> Self {
        let is_monitor = info.name.ends_with(".monitor");
        let default_output = defaults.default_output_monitor();
        let is_default_output = default_output.as_deref() == Some(&info.name);
        let is_default_input = defaults.source.as_deref() == Some(&info.name);

        let mut tags = Vec::new();
        if is_default_output {
            tags.push("default output");
        }
        if is_default_input {
            tags.push("default input");
        }

        let tag_suffix = if tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", tags.join(", "))
        };

        let label = if is_monitor {
            format!("Desktop: {}{}", info.name, tag_suffix)
        } else {
            format!("Input: {}{}", info.name, tag_suffix)
        };

        Self {
            name: info.name,
            label,
            checked,
            is_default_input,
            is_default_output,
            is_monitor,
            driver: info.driver,
            sample_spec: info.sample_spec,
            channel_map: info.channel_map,
            state: info.state,
        }
    }
}

impl FzfSelectable for AudioSourceItem {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_key(&self) -> String {
        self.name.clone()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        self.checked
    }

    fn fzf_preview(&self) -> FzfPreview {
        let kind = if self.is_monitor {
            "Desktop output (monitor)"
        } else {
            "Input source"
        };
        let selected = if self.checked { "Yes" } else { "No" };

        let mut builder = PreviewBuilder::new()
            .header(NerdFont::VolumeUp, &self.name)
            .line(
                colors::TEAL,
                Some(NerdFont::Tag),
                &format!("Type: {}", kind),
            )
            .line(
                colors::TEAL,
                Some(NerdFont::CheckCircle),
                &format!("Selected: {}", selected),
            );

        if self.is_default_output {
            builder = builder.line(colors::GREEN, Some(NerdFont::Star), "Default output source");
        }
        if self.is_default_input {
            builder = builder.line(colors::GREEN, Some(NerdFont::Star), "Default input source");
        }

        if let Some(state) = &self.state {
            builder = builder.line(
                colors::SKY,
                Some(NerdFont::InfoCircle),
                &format!("State: {}", state),
            );
        }
        if let Some(driver) = &self.driver {
            builder = builder.line(
                colors::SKY,
                Some(NerdFont::InfoCircle),
                &format!("Driver: {}", driver),
            );
        }
        if let Some(spec) = &self.sample_spec {
            builder = builder.line(
                colors::SKY,
                Some(NerdFont::InfoCircle),
                &format!("Sample: {}", spec),
            );
        }
        if let Some(map) = &self.channel_map {
            builder = builder.line(
                colors::SKY,
                Some(NerdFont::InfoCircle),
                &format!("Channels: {}", map),
            );
        }

        builder.build()
    }
}

pub struct ScreenRecordAudioSources;

impl ScreenRecordAudioSources {
    pub const KEY: OptionalStringSettingKey = SCREEN_RECORD_AUDIO_SOURCES_KEY;
}

impl Setting for ScreenRecordAudioSources {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("assist.screen_record_audio_sources")
            .title("Screen Recording Audio Sources")
            .icon(NerdFont::VolumeUp)
            .summary("Choose which audio sources to include in screen recordings.\n\nUse Tab or Space to toggle sources, then select Save to apply.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn get_display_state(&self, ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        let stored = ctx.optional_string(Self::KEY);
        let label = if is_audio_sources_default(&stored) {
            "Defaults".to_string()
        } else {
            let selected = parse_audio_source_selection(stored);
            match selected.len() {
                0 => "None".to_string(),
                1 => "1 source".to_string(),
                count => format!("{} sources", count),
            }
        };

        crate::settings::setting::SettingState::Choice {
            current_label: label,
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let stored = ctx.optional_string(Self::KEY);
        let use_defaults = is_audio_sources_default(&stored);
        let sources = match list_audio_sources_short() {
            Ok(list) if !list.is_empty() => list,
            Ok(_) => {
                ctx.show_message("No audio sources found. Is PipeWire/PulseAudio running?");
                return Ok(());
            }
            Err(_) => {
                ctx.show_message("Unable to list audio sources. Ensure pactl is installed.");
                return Ok(());
            }
        };

        let defaults = pactl_defaults().unwrap_or(AudioDefaults {
            sink: None,
            source: None,
        });

        let default_sources = default_source_names(&defaults, &sources);
        let selected = if use_defaults {
            default_sources.clone()
        } else {
            parse_audio_source_selection(stored)
        };

        let selected_set: std::collections::HashSet<String> = selected.iter().cloned().collect();
        let items: Vec<AudioSourceItem> = sources
            .into_iter()
            .map(|source| {
                let checked = selected_set.contains(&source.name);
                AudioSourceItem::new(source, checked, &defaults)
            })
            .collect();

        let mode_hint = if use_defaults {
            "Mode: defaults (dynamic)"
        } else {
            "Mode: custom selection"
        };
        let header_text = format!(
            "Select audio sources to include with recordings.\nTab/Space toggles, Enter confirms.\n{mode_hint}\nUse the action below to follow defaults automatically."
        );
        let header = Header::default(&header_text);

        let defaults_preview = PreviewBuilder::new()
            .header(NerdFont::Star, "Use Default Sources")
            .text("Record the current default desktop output and mic input each time.")
            .blank()
            .line(
                colors::TEAL,
                Some(NerdFont::Target),
                &format!(
                    "Current defaults: {}",
                    if default_sources.is_empty() {
                        "None".to_string()
                    } else {
                        default_sources.join(", ")
                    }
                ),
            )
            .build();

        let default_action_label = if use_defaults {
            "Use default sources (current)"
        } else {
            "Use default sources"
        };
        let actions = vec![ChecklistAction::new("audio_defaults", default_action_label)
            .with_color(colors::GREEN)
            .with_preview(defaults_preview)];

        let selection = FzfWrapper::builder()
            .prompt("Audio sources")
            .header(header)
            .checklist("Save")
            .allow_empty_confirm(true)
            .checklist_actions(actions)
            .checklist_dialog(items)?;

        match selection {
            ChecklistResult::Action(action) if action.key == "audio_defaults" => {
                ctx.set_optional_string(Self::KEY, Some(SCREEN_RECORD_AUDIO_SOURCES_DEFAULT));
                ctx.notify("Screen Recording", "Using default audio sources");
            }
            ChecklistResult::Confirmed(items) => {
                let chosen: Vec<String> = items.into_iter().map(|item| item.name).collect();

                if chosen.is_empty() {
                    ctx.set_optional_string(Self::KEY, None::<String>);
                    ctx.notify("Screen Recording", "Audio sources cleared");
                } else {
                    ctx.set_optional_string(Self::KEY, Some(chosen.join("\n")));
                    ctx.notify(
                        "Screen Recording",
                        &format!("Selected {} audio sources", chosen.len()),
                    );
                }
            }
            _ => {}
        }

        Ok(())
    }
}
