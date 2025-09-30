use crate::common::requirements::{InstallTest, RequiredPackage};
use crate::ui::prelude::Fa;

use super::store::{BoolSettingKey, StringSettingKey};
use super::users;

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
    Command {
        summary: &'static str,
        command: CommandSpec,
        required: &'static [RequiredPackage],
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
    pub requires_reapply: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum CommandStyle {
    Terminal,
    Detached,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub style: CommandStyle,
}

impl CommandSpec {
    pub const fn terminal(program: &'static str, args: &'static [&'static str]) -> Self {
        Self {
            program,
            args,
            style: CommandStyle::Terminal,
        }
    }

    pub const fn detached(program: &'static str, args: &'static [&'static str]) -> Self {
        Self {
            program,
            args,
            style: CommandStyle::Detached,
        }
    }
}

const WIREMIX_PACKAGE: RequiredPackage = RequiredPackage {
    name: "wiremix",
    arch_package_name: Some("wiremix"),
    ubuntu_package_name: None,
    tests: &[InstallTest::WhichSucceeds("wiremix")],
};

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
    SettingCategory {
        id: "audio",
        title: "Audio",
        description: "Sound routing tools and audio behaviour.",
        icon: Fa::VolumeUp,
    },
    SettingCategory {
        id: "system",
        title: "System",
        description: "System administration and user management.",
        icon: Fa::InfoCircle,
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
        requires_reapply: false,
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
        requires_reapply: false,
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
            apply: Some(super::actions::apply_clipboard_manager),
        },
        requires_reapply: false,
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
        requires_reapply: false,
    },
    SettingDefinition {
        id: "audio.wiremix",
        title: "Wiremix",
        category: "audio",
        icon: Fa::VolumeUp,
        breadcrumbs: &["Wiremix"],
        kind: SettingKind::Command {
            summary: "Launch the wiremix TUI to manage PipeWire routing and volumes.",
            command: CommandSpec::terminal("wiremix", &[]),
            required: &[WIREMIX_PACKAGE],
        },
        requires_reapply: false,
    },
    SettingDefinition {
        id: "system.user_management",
        title: "User management",
        category: "system",
        icon: Fa::InfoCircle,
        breadcrumbs: &["User management"],
        kind: SettingKind::Action {
            summary: "Create and update Linux users, groups, and shells.",
            run: users::manage_users,
        },
        requires_reapply: false,
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
