//! Desktop settings (additional)
//!
//! Window layout and other desktop settings.

use anyhow::Result;
use std::process::Command;

use crate::common::audio::{
    AudioDefaults, AudioSourceInfo, default_source_names, list_audio_sources_short, pactl_defaults,
};
use crate::common::compositor::CompositorType;
use crate::common::display::SwayDisplayProvider;
use crate::menu::client::MenuClient;
use crate::menu::protocol::SliderRequest;
use crate::menu_utils::{
    ChecklistResult, FzfSelectable, FzfWrapper, Header, MenuCursor, select_one_with_style_at,
};
use crate::settings::context::SettingsContext;
use crate::settings::deps::{BLUEMAN, PIPER};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::{
    IntSettingKey, OptionalStringSettingKey, SCREEN_RECORD_AUDIO_SOURCES_DEFAULT,
    SCREEN_RECORD_AUDIO_SOURCES_KEY, SCREEN_RECORD_FRAMERATE_KEY, StringSettingKey,
    is_audio_sources_default, parse_audio_source_selection,
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
// Screen Recording
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

pub struct ScreenRecordFramerate;

impl ScreenRecordFramerate {
    const KEY: IntSettingKey = SCREEN_RECORD_FRAMERATE_KEY;
}

const SCREEN_RECORD_MIN_FPS: i64 = 15;
const SCREEN_RECORD_FALLBACK_MAX_FPS: i64 = 240;

impl Setting for ScreenRecordFramerate {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("assist.screen_record_framerate")
            .title("Screen Recording Framerate")
            .icon(NerdFont::Timer)
            .summary("Choose the target FPS for screen recordings.\n\n30 fps is a good default for sharing. Higher values match your display refresh rate for smoother motion.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn get_display_state(&self, ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        let fps = ctx.int(Self::KEY);
        crate::settings::setting::SettingState::Choice {
            current_label: format!("{} fps", fps),
        }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let max_fps = screen_record_max_fps();
        let start_value = ctx.int(Self::KEY).clamp(SCREEN_RECORD_MIN_FPS, max_fps);

        if let Some(value) = run_screen_record_fps_slider(start_value, max_fps)? {
            ctx.set_int(Self::KEY, value);
            ctx.notify(
                "Screen Recording",
                &format!("Recording framerate set to {} fps", value),
            );
        }

        Ok(())
    }
}

fn screen_record_max_fps() -> i64 {
    let detected = detect_display_refresh_hz().unwrap_or(SCREEN_RECORD_FALLBACK_MAX_FPS);
    detected
        .max(SCREEN_RECORD_FRAMERATE_KEY.default)
        .max(SCREEN_RECORD_MIN_FPS + 1)
}

fn detect_display_refresh_hz() -> Option<i64> {
    if !matches!(CompositorType::detect(), CompositorType::Sway) {
        return None;
    }

    let outputs = SwayDisplayProvider::get_outputs_sync().ok()?;
    outputs
        .iter()
        .map(|output| output.current_mode.refresh_hz().round() as i64)
        .max()
}

fn run_screen_record_fps_slider(start_value: i64, max_fps: i64) -> Result<Option<i64>> {
    let client = MenuClient::new();
    client.ensure_server_running()?;

    let request = SliderRequest {
        min: SCREEN_RECORD_MIN_FPS,
        max: max_fps,
        value: Some(start_value),
        step: Some(1),
        big_step: Some(10),
        label: Some(format!("Screen Recording FPS (max {})", max_fps)),
        command: Vec::new(),
    };

    client.slide(request)
}

#[derive(Clone, Copy, PartialEq)]
enum AudioSourceMode {
    Defaults,
    Custom,
}

#[derive(Clone)]
struct AudioSourceModeItem {
    mode: AudioSourceMode,
    is_current: bool,
    default_sources: Vec<String>,
    selected_sources: Vec<String>,
}

impl AudioSourceModeItem {
    fn new(
        mode: AudioSourceMode,
        is_current: bool,
        default_sources: &[String],
        selected_sources: &[String],
    ) -> Self {
        Self {
            mode,
            is_current,
            default_sources: default_sources.to_vec(),
            selected_sources: selected_sources.to_vec(),
        }
    }
}

impl FzfSelectable for AudioSourceModeItem {
    fn fzf_display_text(&self) -> String {
        let icon = if self.is_current {
            format_icon_colored(NerdFont::CheckSquare, colors::GREEN)
        } else {
            format_icon_colored(NerdFont::Square, colors::OVERLAY1)
        };
        match self.mode {
            AudioSourceMode::Defaults => format!(
                "{} Auto-detect sources ({})",
                icon,
                format_source_count(self.default_sources.len())
            ),
            AudioSourceMode::Custom => format!(
                "{} Select sources ({})",
                icon,
                format_source_count(self.selected_sources.len())
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self.mode {
            AudioSourceMode::Defaults => PreviewBuilder::new()
                .header(NerdFont::Star, "Auto-detect sources")
                .text("Record the current output and mic each time.")
                .blank()
                .line(
                    colors::TEAL,
                    Some(NerdFont::Target),
                    &format!(
                        "Auto-detected sources: {}",
                        format_sources_list(&self.default_sources)
                    ),
                )
                .build(),
            AudioSourceMode::Custom => PreviewBuilder::new()
                .header(NerdFont::List, "Select sources")
                .text("Choose specific sources to include with recordings.")
                .blank()
                .line(
                    colors::TEAL,
                    Some(NerdFont::CheckCircle),
                    &format!("Selected: {}", format_sources_list(&self.selected_sources)),
                )
                .line(
                    colors::SKY,
                    Some(NerdFont::InfoCircle),
                    "Auto-detect is ignored while using a custom list.",
                )
                .build(),
        }
    }

    fn fzf_key(&self) -> String {
        match self.mode {
            AudioSourceMode::Defaults => "auto".to_string(),
            AudioSourceMode::Custom => "custom".to_string(),
        }
    }
}

fn format_source_count(count: usize) -> String {
    match count {
        0 => "none".to_string(),
        1 => "1 source".to_string(),
        _ => format!("{} sources", count),
    }
}

fn format_sources_list(sources: &[String]) -> String {
    if sources.is_empty() {
        "None".to_string()
    } else {
        sources.join(", ")
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
            .summary("Choose auto-detect or a custom list of audio sources for screen recordings.\n\nAuto-detect follows your current output and mic; custom selection overrides it.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn get_display_state(&self, ctx: &SettingsContext) -> crate::settings::setting::SettingState {
        let stored = ctx.optional_string(Self::KEY);
        let label = if is_audio_sources_default(&stored) {
            "Auto-detect".to_string()
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
        let sources = match load_audio_sources(ctx)? {
            Some(list) => list,
            None => return Ok(()),
        };
        let defaults = load_audio_defaults();
        let default_sources = default_source_names(&defaults, &sources);
        let (mut mode, mut custom_selected) = load_audio_source_state(stored);

        run_audio_source_mode_menu(
            ctx,
            &sources,
            &defaults,
            &default_sources,
            &mut mode,
            &mut custom_selected,
        )
    }
}

fn load_audio_sources(ctx: &SettingsContext) -> Result<Option<Vec<AudioSourceInfo>>> {
    match list_audio_sources_short() {
        Ok(list) if !list.is_empty() => Ok(Some(list)),
        Ok(_) => {
            ctx.show_message("No audio sources found. Is PipeWire/PulseAudio running?");
            Ok(None)
        }
        Err(_) => {
            ctx.show_message("Unable to list audio sources. Ensure pactl is installed.");
            Ok(None)
        }
    }
}

fn load_audio_defaults() -> AudioDefaults {
    pactl_defaults().unwrap_or(AudioDefaults {
        sink: None,
        source: None,
    })
}

fn load_audio_source_state(stored: Option<String>) -> (AudioSourceMode, Vec<String>) {
    let use_defaults = is_audio_sources_default(&stored);
    let mode = if use_defaults {
        AudioSourceMode::Defaults
    } else {
        AudioSourceMode::Custom
    };
    let custom_selected = if use_defaults {
        Vec::new()
    } else {
        parse_audio_source_selection(stored)
    };
    (mode, custom_selected)
}

fn run_audio_source_mode_menu(
    ctx: &mut SettingsContext,
    sources: &[AudioSourceInfo],
    defaults: &AudioDefaults,
    default_sources: &[String],
    mode: &mut AudioSourceMode,
    custom_selected: &mut Vec<String>,
) -> Result<()> {
    loop {
        let items = build_audio_source_mode_items(*mode, default_sources, custom_selected);
        let initial_index = mode_to_index(*mode);

        let selection = select_one_with_style_at(items.clone(), initial_index)?;
        let Some(choice) = selection else {
            break;
        };

        match choice.mode {
            AudioSourceMode::Defaults => {
                *mode = AudioSourceMode::Defaults;
                apply_default_audio_sources(ctx);
            }
            AudioSourceMode::Custom => {
                *mode = AudioSourceMode::Custom;
                handle_custom_audio_sources(
                    ctx,
                    sources,
                    defaults,
                    default_sources,
                    custom_selected,
                )?;
            }
        }
    }

    Ok(())
}

fn build_audio_source_mode_items(
    mode: AudioSourceMode,
    default_sources: &[String],
    custom_selected: &[String],
) -> Vec<AudioSourceModeItem> {
    vec![
        AudioSourceModeItem::new(
            AudioSourceMode::Defaults,
            matches!(mode, AudioSourceMode::Defaults),
            default_sources,
            custom_selected,
        ),
        AudioSourceModeItem::new(
            AudioSourceMode::Custom,
            matches!(mode, AudioSourceMode::Custom),
            default_sources,
            custom_selected,
        ),
    ]
}

fn mode_to_index(mode: AudioSourceMode) -> Option<usize> {
    match mode {
        AudioSourceMode::Defaults => Some(0),
        AudioSourceMode::Custom => Some(1),
    }
}

fn apply_default_audio_sources(ctx: &mut SettingsContext) {
    ctx.set_optional_string(
        ScreenRecordAudioSources::KEY,
        Some(SCREEN_RECORD_AUDIO_SOURCES_DEFAULT),
    );
    ctx.notify("Screen Recording", "Using auto-detect audio sources");
}

fn handle_custom_audio_sources(
    ctx: &mut SettingsContext,
    sources: &[AudioSourceInfo],
    defaults: &AudioDefaults,
    default_sources: &[String],
    custom_selected: &mut Vec<String>,
) -> Result<()> {
    let seed_selection = seed_custom_audio_selection(custom_selected, default_sources);
    let items = build_audio_source_checklist(sources, defaults, &seed_selection);
    let header = build_custom_audio_header(default_sources);

    let selection = FzfWrapper::builder()
        .prompt("Audio sources")
        .header(header)
        .checklist("Save")
        .allow_empty_confirm(true)
        .checklist_dialog(items)?;

    match selection {
        ChecklistResult::Confirmed(items) => {
            let chosen: Vec<String> = items.into_iter().map(|item| item.name).collect();
            *custom_selected = chosen.clone();
            apply_custom_audio_selection(ctx, &chosen);
        }
        ChecklistResult::Cancelled => {}
        ChecklistResult::Action(_) => {}
    }

    Ok(())
}

fn seed_custom_audio_selection(
    custom_selected: &[String],
    default_sources: &[String],
) -> Vec<String> {
    if custom_selected.is_empty() {
        default_sources.to_vec()
    } else {
        custom_selected.to_vec()
    }
}

fn build_audio_source_checklist(
    sources: &[AudioSourceInfo],
    defaults: &AudioDefaults,
    seed_selection: &[String],
) -> Vec<AudioSourceItem> {
    let selected_set: std::collections::HashSet<String> = seed_selection.iter().cloned().collect();
    sources
        .iter()
        .cloned()
        .map(|source| {
            let checked = selected_set.contains(&source.name);
            AudioSourceItem::new(source, checked, defaults)
        })
        .collect()
}

fn build_custom_audio_header(default_sources: &[String]) -> Header {
    let header_text = format!(
        "Select audio sources to include with recordings.\nEnter toggles, select Save to confirm.\nAuto-detected sources (ignored in custom mode): {}",
        format_sources_list(default_sources)
    );
    Header::default(&header_text)
}

fn apply_custom_audio_selection(ctx: &mut SettingsContext, chosen: &[String]) {
    if chosen.is_empty() {
        ctx.set_optional_string(ScreenRecordAudioSources::KEY, None::<String>);
        ctx.notify("Screen Recording", "Audio sources cleared");
    } else {
        ctx.set_optional_string(ScreenRecordAudioSources::KEY, Some(chosen.join("\n")));
        ctx.notify(
            "Screen Recording",
            &format!("Selected {} audio sources", chosen.len()),
        );
    }
}
