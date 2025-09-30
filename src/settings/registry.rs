use crate::ui::prelude::Fa;

use super::store::{BoolSettingKey, StringSettingKey};

#[derive(Debug, Clone)]
pub struct SettingCategory {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: Fa,
}

#[derive(Debug, Clone, Copy)]
pub struct SettingOption {
    pub value: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone)]
pub enum SettingKind {
    Toggle {
        key: BoolSettingKey,
        summary: &'static str,
        apply: Option<fn(&mut super::SettingsContext, bool) -> anyhow::Result<()>>,
    },
    Choice {
        key: StringSettingKey,
        summary: &'static str,
        options: &'static [SettingOption],
        apply: Option<fn(&mut super::SettingsContext, &SettingOption) -> anyhow::Result<()>>,
    },
    Action {
        summary: &'static str,
        run: fn(&mut super::SettingsContext) -> anyhow::Result<()>,
    },
}

#[derive(Debug, Clone)]
pub struct SettingDefinition {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub icon: Fa,
    pub breadcrumbs: &'static [&'static str],
    pub kind: SettingKind,
}

pub const CATEGORIES: &[SettingCategory] = &[
    SettingCategory {
        id: "appearance",
        title: "Appearance",
        description: "Theme and visual presentation of the desktop.",
        icon: Fa::LightbulbO,
    },
    SettingCategory {
        id: "desktop",
        title: "Desktop",
        description: "Interactive desktop behaviour and helpers.",
        icon: Fa::Desktop,
    },
    SettingCategory {
        id: "workspace",
        title: "Workspace",
        description: "Window manager defaults and layout preferences.",
        icon: Fa::Folder,
    },
];

pub const SETTINGS: &[SettingDefinition] = &[
    SettingDefinition {
        id: "appearance.autotheming",
        title: "Autotheming",
        category: "appearance",
        icon: Fa::InfoCircle,
        breadcrumbs: &["Autotheming"],
        kind: SettingKind::Toggle {
            key: BoolSettingKey::new("appearance.autotheming", true),
            summary: "Enable instantOS theming (disable for custom GTK themes).",
            apply: None,
        },
    },
    SettingDefinition {
        id: "appearance.animations",
        title: "Animations",
        category: "appearance",
        icon: Fa::Check,
        breadcrumbs: &["Animations"],
        kind: SettingKind::Toggle {
            key: BoolSettingKey::new("appearance.animations", true),
            summary: "Controls desktop animation effects.",
            apply: None,
        },
    },
    SettingDefinition {
        id: "desktop.clipboard",
        title: "Clipboard manager",
        category: "desktop",
        icon: Fa::Folder,
        breadcrumbs: &["Clipboard manager"],
        kind: SettingKind::Toggle {
            key: BoolSettingKey::new("desktop.clipboard", true),
            summary: "Toggle the clipmenud clipboard manager.",
            apply: Some(super::apply_clipboard_manager),
        },
    },
    SettingDefinition {
        id: "workspace.layout",
        title: "Default layout",
        category: "workspace",
        icon: Fa::List,
        breadcrumbs: &["Default layout"],
        kind: SettingKind::Choice {
            key: StringSettingKey::new("workspace.layout", "tile"),
            summary: "Select the default instantWM layout.",
            options: &[
                SettingOption {
                    value: "tile",
                    label: "Tile",
                    description: "Classic tiling layout",
                },
                SettingOption {
                    value: "grid",
                    label: "Grid",
                    description: "Balanced grid layout",
                },
                SettingOption {
                    value: "float",
                    label: "Float",
                    description: "Floating window placement",
                },
                SettingOption {
                    value: "monocle",
                    label: "Monocle",
                    description: "Single maximized window",
                },
                SettingOption {
                    value: "tcl",
                    label: "TCL",
                    description: "Three column layout",
                },
                SettingOption {
                    value: "deck",
                    label: "Deck",
                    description: "Primary window with stack",
                },
                SettingOption {
                    value: "overviewlayout",
                    label: "Overview",
                    description: "Overview of workspaces",
                },
                SettingOption {
                    value: "bstack",
                    label: "Bottom stack",
                    description: "Primary window with bottom stack",
                },
                SettingOption {
                    value: "bstackhoriz",
                    label: "Bottom stack horizontal",
                    description: "Bottom stack with horizontal split",
                },
            ],
            apply: None,
        },
    },
];

pub fn category_by_id(id: &str) -> Option<&'static SettingCategory> {
    CATEGORIES.iter().find(|category| category.id == id)
}

pub fn settings_for_category(id: &str) -> Vec<&'static SettingDefinition> {
    SETTINGS
        .iter()
        .filter(|setting| setting.category == id)
        .collect()
}
