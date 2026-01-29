//! Desktop settings (additional)
//!
//! Window layout and other desktop settings.

use anyhow::{Context, Result};
use std::process::Command;

use crate::common::compositor::CompositorType;
use crate::menu_utils::{select_one_with_style_at, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::settings::context::SettingsContext;
use crate::settings::deps::{BLUEMAN, PIPER};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{
    OptionalStringSettingKey, StringSettingKey, SCREEN_RECORD_AUDIO_SOURCES_KEY,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::prelude::*;

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
}

impl AudioSourceItem {
    fn new(name: &str, checked: bool) -> Self {
        let label = if name.ends_with(".monitor") {
            format!("Desktop: {name}")
        } else {
            format!("Input: {name}")
        };

        Self {
            name: name.to_string(),
            label,
            checked,
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
}

fn parse_audio_sources(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

fn list_audio_sources() -> Result<Vec<String>> {
    let output = Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .context("Failed to run pactl list sources")?;

    if !output.status.success() {
        anyhow::bail!("pactl list sources failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sources = Vec::new();

    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let _ = parts.next();
        if let Some(name) = parts.next() {
            sources.push(name.to_string());
        }
    }

    Ok(sources)
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
        let selected = parse_audio_sources(ctx.optional_string(Self::KEY));
        let label = match selected.len() {
            0 => "None".to_string(),
            1 => "1 source".to_string(),
            count => format!("{} sources", count),
        };

        crate::settings::setting::SettingState::Choice {
            current_label: label,
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let selected = parse_audio_sources(ctx.optional_string(Self::KEY));
        let sources = match list_audio_sources() {
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

        let selected_set: std::collections::HashSet<String> = selected.iter().cloned().collect();
        let items: Vec<AudioSourceItem> = sources
            .iter()
            .map(|source| AudioSourceItem::new(source, selected_set.contains(source)))
            .collect();

        let header = Header::default(
            "Select audio sources to include with recordings.\nTab/Space toggles, Enter confirms.",
        );

        let selection = FzfWrapper::builder()
            .prompt("Audio sources")
            .header(header)
            .checklist("Save")
            .allow_empty_confirm(true)
            .checklist_dialog(items)?;

        if let crate::menu_utils::ChecklistResult::Confirmed(items) = selection {
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

        Ok(())
    }
}
