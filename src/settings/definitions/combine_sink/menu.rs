use crate::menu_utils::FzfSelectable;
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

use super::config::get_current_sink_name;

/// Menu action types with their display and preview information
#[derive(Clone)]
pub(super) enum MenuAction {
    Remove,
    ChangeDevices,
    Rename,
    SetAsDefault,
    Enable,
    Back,
}

#[derive(Clone)]
pub(super) struct MenuItem {
    pub(super) action: MenuAction,
    label: String,
    icon: String,
}

impl MenuItem {
    pub(super) fn new(
        action: MenuAction,
        label: impl Into<String>,
        icon: impl Into<String>,
    ) -> Self {
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
pub(super) struct MenuItemWithPreview {
    pub(super) item: MenuItem,
    preview: FzfPreview,
}

impl MenuItemWithPreview {
    pub(super) fn new(item: MenuItem, preview: FzfPreview) -> Self {
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

pub(super) fn build_menu_items(
    currently_enabled: bool,
    is_default: bool,
    device_count: usize,
    current_name: &str,
) -> Vec<MenuItem> {
    let mut items = Vec::new();

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

    items
}

pub(super) fn build_header_text(
    currently_enabled: bool,
    current_name: &str,
    device_count: usize,
) -> String {
    if currently_enabled {
        format!(
            "Combined Audio Sink: {} (active)\n{} devices",
            current_name, device_count
        )
    } else {
        "Combined Audio Sink: Not active".to_string()
    }
}

pub(super) fn build_items_with_previews(
    items: Vec<MenuItem>,
    currently_enabled: bool,
    is_default: bool,
    device_list: &[String],
    config_path_display: &str,
) -> Vec<MenuItemWithPreview> {
    items
        .into_iter()
        .map(|item| {
            let preview = item.preview(
                currently_enabled,
                is_default,
                device_list,
                config_path_display,
            );
            MenuItemWithPreview::new(item, preview)
        })
        .collect()
}
