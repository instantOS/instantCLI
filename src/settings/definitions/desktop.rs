//! Desktop settings (additional)
//!
//! Window layout and other desktop settings.

use anyhow::Result;
use std::process::Command;

use crate::common::compositor::CompositorType;
use crate::menu_utils::select_one_with_style_at;
use crate::menu_utils::FzfSelectable;
use crate::menu_utils::MenuCursor;
use crate::settings::context::SettingsContext;
use crate::settings::deps::{BLUEMAN, PIPER};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{
    BoolSettingKey, StringSettingKey, SCREEN_RECORD_AUDIO_KEY, SCREEN_RECORD_DESKTOP_AUDIO_KEY,
    SCREEN_RECORD_MIC_AUDIO_KEY,
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
// Screen Recording Audio Toggle
// ============================================================================

pub struct ScreenRecordAudio;

impl ScreenRecordAudio {
    pub const KEY: BoolSettingKey = SCREEN_RECORD_AUDIO_KEY;
}

impl Setting for ScreenRecordAudio {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("assist.screen_record_audio")
            .title("Screen Recording Audio")
            .icon(NerdFont::VolumeUp)
            .summary("Enable audio capture when screen recording with ins assist.\n\nChoose which sources to include below (desktop audio and/or microphone).")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn get_display_state(&self, ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        crate::settings::setting::SettingState::Toggle {
            enabled: ctx.bool(Self::KEY),
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        ctx.set_bool(Self::KEY, !current);

        if !current {
            ctx.notify("Screen Recording", "Audio capture enabled");
        } else {
            ctx.notify("Screen Recording", "Audio capture disabled");
        }

        Ok(())
    }
}

// ============================================================================
// Screen Recording Desktop Audio Toggle
// ============================================================================

pub struct ScreenRecordDesktopAudio;

impl ScreenRecordDesktopAudio {
    pub const KEY: BoolSettingKey = SCREEN_RECORD_DESKTOP_AUDIO_KEY;
}

impl Setting for ScreenRecordDesktopAudio {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("assist.screen_record_desktop_audio")
            .title("Screen Recording Desktop Audio")
            .icon(NerdFont::VolumeUp)
            .summary("Include desktop (system output) audio in screen recordings.\n\nUses the default sink monitor from PipeWire/PulseAudio.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn get_display_state(&self, ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        crate::settings::setting::SettingState::Toggle {
            enabled: ctx.bool(Self::KEY),
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        ctx.set_bool(Self::KEY, !current);

        if !current {
            ctx.notify("Screen Recording", "Desktop audio capture enabled");
        } else {
            ctx.notify("Screen Recording", "Desktop audio capture disabled");
        }

        Ok(())
    }
}

// ============================================================================
// Screen Recording Microphone Audio Toggle
// ============================================================================

pub struct ScreenRecordMicrophoneAudio;

impl ScreenRecordMicrophoneAudio {
    pub const KEY: BoolSettingKey = SCREEN_RECORD_MIC_AUDIO_KEY;
}

impl Setting for ScreenRecordMicrophoneAudio {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("assist.screen_record_mic_audio")
            .title("Screen Recording Microphone Audio")
            .icon(NerdFont::VolumeDown)
            .summary("Include microphone input in screen recordings.\n\nUses the default input source from PipeWire/PulseAudio.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn get_display_state(&self, ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        crate::settings::setting::SettingState::Toggle {
            enabled: ctx.bool(Self::KEY),
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        ctx.set_bool(Self::KEY, !current);

        if !current {
            ctx.notify("Screen Recording", "Microphone audio capture enabled");
        } else {
            ctx.notify("Screen Recording", "Microphone audio capture disabled");
        }

        Ok(())
    }
}
