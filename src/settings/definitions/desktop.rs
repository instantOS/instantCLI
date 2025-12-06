//! Desktop settings (additional)
//!
//! Window layout and other desktop settings.

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
use crate::ui::prelude::*;

// ============================================================================
// Window Layout
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
        SettingMetadata {
            id: "desktop.layout",
            title: "Window Layout",
            category: Category::Desktop,
            icon: NerdFont::List,
            breadcrumbs: &["Window Layout"],
            summary: "Choose how windows are arranged on your screen by default.\n\nYou can always change the layout temporarily with keyboard shortcuts.",
            requires_reapply: false,
            requirements: &[],
        }
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

        match result {
            FzfResult::Selected(layout) => {
                ctx.set_string(Self::KEY, layout.value);
                ctx.notify("Window Layout", &format!("Set to: {}", layout.label));
            }
            _ => {}
        }

        Ok(())
    }
}

inventory::submit! { &WindowLayout as &'static dyn Setting }

// ============================================================================
// Gaming Mouse
// ============================================================================

pub struct GamingMouse;

impl Setting for GamingMouse {
    fn metadata(&self) -> SettingMetadata {
        use crate::common::requirements::PIPER_PACKAGE;
        SettingMetadata {
            id: "mouse.gaming",
            title: "Gaming Mouse Customization",
            category: Category::Mouse,
            icon: NerdFont::Mouse,
            breadcrumbs: &["Gaming Mouse Customization"],
            summary: "Configure gaming mice with customizable buttons, RGB lighting, and DPI settings.\n\nUses Piper to configure Logitech and other gaming mice supported by libratbag.",
            requires_reapply: false,
            requirements: &[crate::settings::setting::Requirement::Package(
                PIPER_PACKAGE,
            )],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Launching Piper...");
        std::process::Command::new("piper").spawn()?;
        ctx.emit_success("settings.command.completed", "Launched Piper");
        Ok(())
    }
}

inventory::submit! { &GamingMouse as &'static dyn Setting }

// ============================================================================
// Bluetooth Manager
// ============================================================================

pub struct BluetoothManager;

impl Setting for BluetoothManager {
    fn metadata(&self) -> SettingMetadata {
        use crate::common::requirements::BLUEMAN_PACKAGE;
        SettingMetadata {
            id: "bluetooth.manager",
            title: "Manage Devices",
            category: Category::Bluetooth,
            icon: NerdFont::Settings,
            breadcrumbs: &["Manage Devices"],
            summary: "Pair new devices and manage connected Bluetooth devices.\n\nUse this to connect headphones, speakers, keyboards, mice, and other wireless devices.",
            requires_reapply: false,
            requirements: &[crate::settings::setting::Requirement::Package(
                BLUEMAN_PACKAGE,
            )],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Command
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        ctx.emit_info("settings.command.launching", "Launching Blueman Manager...");
        std::process::Command::new("blueman-manager").spawn()?;
        ctx.emit_success("settings.command.completed", "Launched Blueman Manager");
        Ok(())
    }
}

inventory::submit! { &BluetoothManager as &'static dyn Setting }
