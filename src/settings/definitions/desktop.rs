//! Desktop settings (additional)
//!
//! Window layout and other desktop settings.

use anyhow::Result;

use crate::common::requirements::{BLUEMAN_PACKAGE, PIPER_PACKAGE};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
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

impl FzfSelectable for LayoutChoice {
    fn fzf_display_text(&self) -> String {
        format!("{}: {}", self.label, self.description)
    }

    fn fzf_key(&self) -> String {
        self.value.to_string()
    }
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
            .category(Category::Desktop)
            .icon(NerdFont::List)
            .breadcrumbs(&["Window Layout"])
            .summary("Choose how windows are arranged on your screen by default.\n\nYou can always change the layout temporarily with keyboard shortcuts.")
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

        let result = FzfWrapper::builder()
            .header("Select Window Layout")
            .prompt("Layout > ")
            .initial_index(initial_index)
            .select(LAYOUT_OPTIONS.to_vec())?;

        if let FzfResult::Selected(layout) = result {
            ctx.set_string(Self::KEY, layout.value);
            ctx.notify("Window Layout", &format!("Set to: {}", layout.label));
        }

        Ok(())
    }
}

inventory::submit! { &WindowLayout as &'static dyn Setting }

// ============================================================================
// Gaming Mouse (GUI app)
// ============================================================================

gui_command_setting!(
    GamingMouse,
    "mouse.gaming",
    "Gaming Mouse Customization",
    Category::Mouse,
    NerdFont::Mouse,
    "Configure gaming mice with customizable buttons, RGB lighting, and DPI settings.\n\nUses Piper to configure Logitech and other gaming mice supported by libratbag.",
    "piper",
    PIPER_PACKAGE
);

// ============================================================================
// Bluetooth Manager (GUI app)
// ============================================================================

gui_command_setting!(
    BluetoothManager,
    "bluetooth.manager",
    "Manage Devices",
    Category::Bluetooth,
    NerdFont::Settings,
    "Pair new devices and manage connected Bluetooth devices.\n\nUse this to connect headphones, speakers, keyboards, mice, and other wireless devices.",
    "blueman-manager",
    BLUEMAN_PACKAGE
);
