use crate::common::requirements::{InstallTest, RequiredPackage};
use crate::ui::prelude::NerdFont;

use super::printer;
use super::store::{BoolSettingKey, StringSettingKey};
use super::users;

/// A requirement that must be satisfied before a setting can be used
#[derive(Debug, Clone)]
pub enum SettingRequirement {
    /// Requires a package to be installed
    Package(RequiredPackage),
    /// Requires a custom condition to be true (e.g., service running, hardware present)
    Condition {
        description: &'static str,
        check: fn() -> bool,
        resolve_hint: &'static str,
    },
}

impl SettingRequirement {
    /// Check if this requirement is currently satisfied
    pub fn is_satisfied(&self) -> bool {
        match self {
            SettingRequirement::Package(pkg) => pkg.is_installed(),
            SettingRequirement::Condition { check, .. } => check(),
        }
    }

    /// Get a human-readable description of this requirement
    pub fn description(&self) -> &str {
        match self {
            SettingRequirement::Package(pkg) => pkg.name,
            SettingRequirement::Condition { description, .. } => description,
        }
    }

    /// Get a hint for how to resolve this requirement
    pub fn resolve_hint(&self) -> String {
        match self {
            SettingRequirement::Package(pkg) => pkg.install_hint(),
            SettingRequirement::Condition { resolve_hint, .. } => resolve_hint.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingCategory {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: NerdFont,
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
    },
}

#[derive(Debug, Clone)]
pub struct SettingDefinition {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub icon: NerdFont,
    pub breadcrumbs: &'static [&'static str],
    pub kind: SettingKind,
    pub requires_reapply: bool,
    /// Requirements that must be satisfied before this setting can be used
    pub requirements: &'static [SettingRequirement],
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

pub const UDISKIE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "udiskie",
    arch_package_name: Some("udiskie"),
    ubuntu_package_name: Some("udiskie"),
    tests: &[InstallTest::WhichSucceeds("udiskie")],
};

pub const GNOME_DISKS_PACKAGE: RequiredPackage = RequiredPackage {
    name: "GNOME Disks",
    arch_package_name: Some("gnome-disk-utility"),
    ubuntu_package_name: Some("gnome-disk-utility"),
    tests: &[InstallTest::WhichSucceeds("gnome-disks")],
};

pub const GPARTED_PACKAGE: RequiredPackage = RequiredPackage {
    name: "GParted",
    arch_package_name: Some("gparted"),
    ubuntu_package_name: Some("gparted"),
    tests: &[InstallTest::WhichSucceeds("gparted")],
};

pub const FASTFETCH_PACKAGE: RequiredPackage = RequiredPackage {
    name: "fastfetch",
    arch_package_name: Some("fastfetch"),
    ubuntu_package_name: Some("fastfetch"),
    tests: &[InstallTest::WhichSucceeds("fastfetch")],
};

pub const TOPGRADE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "topgrade",
    arch_package_name: Some("topgrade"),
    ubuntu_package_name: None,
    tests: &[InstallTest::WhichSucceeds("topgrade")],
};

pub const BLUETOOTH_SERVICE_KEY: BoolSettingKey = BoolSettingKey::new("bluetooth.service", false);
pub const BLUETOOTH_HARDWARE_OVERRIDE_KEY: BoolSettingKey =
    BoolSettingKey::new("bluetooth.hardware_override", false);

pub const UDISKIE_AUTOMOUNT_KEY: BoolSettingKey = BoolSettingKey::new("storage.udiskie", false);

pub const BLUEZ_PACKAGE: RequiredPackage = RequiredPackage {
    name: "BlueZ bluetooth daemon",
    arch_package_name: Some("bluez"),
    ubuntu_package_name: Some("bluez"),
    tests: &[
        InstallTest::WhichSucceeds("bluetoothd"),
        InstallTest::FileExists("/usr/lib/systemd/system/bluetooth.service"),
    ],
};

pub const BLUEZ_UTILS_PACKAGE: RequiredPackage = RequiredPackage {
    name: "BlueZ utilities",
    arch_package_name: Some("bluez-utils"),
    ubuntu_package_name: Some("bluez"),
    tests: &[InstallTest::WhichSucceeds("bluetoothctl")],
};

pub const BLUEMAN_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Blueman applet",
    arch_package_name: Some("blueman"),
    ubuntu_package_name: Some("blueman"),
    tests: &[InstallTest::WhichSucceeds("blueman-applet")],
};

pub const BLUETOOTH_CORE_PACKAGES: [RequiredPackage; 2] = [BLUEZ_PACKAGE, BLUEZ_UTILS_PACKAGE];

pub const COCKPIT_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Cockpit",
    arch_package_name: Some("cockpit"),
    ubuntu_package_name: Some("cockpit"),
    tests: &[
        InstallTest::FileExists("/usr/lib/systemd/system/cockpit.socket"),
        InstallTest::WhichSucceeds("cockpit-bridge"),
    ],
};

pub const CHROMIUM_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Chromium browser",
    arch_package_name: Some("chromium"),
    ubuntu_package_name: Some("chromium-browser"),
    tests: &[InstallTest::WhichSucceeds("chromium")],
};

pub const COCKPIT_PACKAGES: [RequiredPackage; 2] = [COCKPIT_PACKAGE, CHROMIUM_PACKAGE];

/// Check if the bluetooth service is currently active
fn bluetooth_service_active() -> bool {
    crate::common::systemd::SystemdManager::system().is_active("bluetooth")
}

// Requirement definitions for common use cases
pub const WIREMIX_REQUIREMENT: SettingRequirement = SettingRequirement::Package(WIREMIX_PACKAGE);

pub const BLUETOOTH_SERVICE_REQUIREMENT: SettingRequirement = SettingRequirement::Condition {
    description: "Bluetooth service must be running",
    check: bluetooth_service_active,
    resolve_hint: "Enable Bluetooth in Settings > Bluetooth > Enable Bluetooth",
};

pub const BLUETOOTH_MANAGER_REQUIREMENTS: [SettingRequirement; 4] = [
    SettingRequirement::Package(BLUEZ_PACKAGE),
    SettingRequirement::Package(BLUEZ_UTILS_PACKAGE),
    SettingRequirement::Package(BLUEMAN_PACKAGE),
    BLUETOOTH_SERVICE_REQUIREMENT,
];

pub const CATEGORIES: &[SettingCategory] = &[
    SettingCategory {
        id: "appearance",
        title: "Appearance",
        description: "Theme and visual presentation of the desktop.",
        icon: NerdFont::Lightbulb,
    },
    SettingCategory {
        id: "desktop",
        title: "Desktop",
        description: "Interactive desktop behaviour and helpers.",
        icon: NerdFont::Desktop,
    },
    SettingCategory {
        id: "workspace",
        title: "Workspace",
        description: "Window manager defaults and layout preferences.",
        icon: NerdFont::Folder,
    },
    SettingCategory {
        id: "audio",
        title: "Audio",
        description: "Sound routing tools and audio behaviour.",
        icon: NerdFont::VolumeUp,
    },
    SettingCategory {
        id: "apps",
        title: "Applications",
        description: "Default applications and file associations.",
        icon: NerdFont::Package,
    },
    SettingCategory {
        id: "network",
        title: "Network",
        description: "Internet connection and network settings.",
        icon: NerdFont::Wifi,
    },
    SettingCategory {
        id: "bluetooth",
        title: "Bluetooth",
        description: "Pair and manage Bluetooth devices.",
        icon: NerdFont::Bluetooth,
    },
    SettingCategory {
        id: "storage",
        title: "Storage",
        description: "Disk management and auto-mounting.",
        icon: NerdFont::Save,
    },
    SettingCategory {
        id: "printers",
        title: "Printers",
        description: "Discover, configure, and manage printers.",
        icon: NerdFont::Printer,
    },
    SettingCategory {
        id: "users",
        title: "Users",
        description: "Create and manage user accounts.",
        icon: NerdFont::Users,
    },
    SettingCategory {
        id: "system",
        title: "System",
        description: "System administration and maintenance.",
        icon: NerdFont::Server,
    },
];

pub const SETTINGS: &[SettingDefinition] = &[
    SettingDefinition {
        id: "appearance.autotheming",
        title: "Automatic Theming",
        category: "appearance",
        icon: NerdFont::Palette,
        breadcrumbs: &["Automatic Theming"],
        kind: SettingKind::Toggle {
            key: BoolSettingKey::new("appearance.autotheming", true),
            summary: "Automatically apply instantOS color themes to applications.\n\nDisable this if you want to use your own custom themes.\n\nNote: Placeholder only; changing this setting currently has no effect.",
            apply: None,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "appearance.animations",
        title: "Animations",
        category: "appearance",
        icon: NerdFont::Magic,
        breadcrumbs: &["Animations"],
        kind: SettingKind::Toggle {
            key: BoolSettingKey::new("appearance.animations", true),
            summary: "Enable smooth animations and visual effects on the desktop.\n\nDisable for better performance on older hardware.\n\nNote: Placeholder only; changing this setting currently has no effect.",
            apply: None,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "desktop.clipboard",
        title: "Clipboard History",
        category: "desktop",
        icon: NerdFont::Clipboard,
        breadcrumbs: &["Clipboard History"],
        kind: SettingKind::Toggle {
            key: BoolSettingKey::new("desktop.clipboard", true),
            summary: "Remember your copy/paste history so you can access previously copied items.\n\nWhen enabled, you can paste from your clipboard history instead of just the last copied item.",
            apply: Some(super::actions::apply_clipboard_manager),
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "workspace.layout",
        title: "Window Layout",
        category: "workspace",
        icon: NerdFont::List,
        breadcrumbs: &["Window Layout"],
        kind: SettingKind::Choice {
            key: StringSettingKey::new("workspace.layout", "tile"),
            summary: "Choose how windows are arranged on your screen by default.\n\nYou can always change the layout temporarily with keyboard shortcuts.",
            options: &[
                SettingOption {
                    value: "tile",
                    label: "Tile",
                    description: "Windows split the screen side-by-side (recommended for most users)",
                },
                SettingOption {
                    value: "grid",
                    label: "Grid",
                    description: "Windows arranged in an even grid pattern",
                },
                SettingOption {
                    value: "float",
                    label: "Float",
                    description: "Windows can be freely moved and resized (like Windows/macOS)",
                },
                SettingOption {
                    value: "monocle",
                    label: "Monocle",
                    description: "One window fills the entire screen at a time",
                },
                SettingOption {
                    value: "tcl",
                    label: "Three Columns",
                    description: "Main window in center, others on sides",
                },
                SettingOption {
                    value: "deck",
                    label: "Deck",
                    description: "Large main window with smaller windows stacked on the side",
                },
                SettingOption {
                    value: "overviewlayout",
                    label: "Overview",
                    description: "See all your workspaces at once",
                },
                SettingOption {
                    value: "bstack",
                    label: "Bottom Stack",
                    description: "Main window on top, others stacked below",
                },
                SettingOption {
                    value: "bstackhoriz",
                    label: "Bottom Stack (Horizontal)",
                    description: "Main window on top, others arranged horizontally below",
                },
            ],
            apply: None,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "audio.wiremix",
        title: "General audio settings",
        category: "audio",
        icon: NerdFont::Settings,
        breadcrumbs: &["General audio settings"],
        kind: SettingKind::Command {
            summary: "Launch wiremix TUI to manage PipeWire routing and volumes.",
            command: CommandSpec::terminal("wiremix", &[]),
        },
        requires_reapply: false,
        requirements: &[WIREMIX_REQUIREMENT],
    },
    SettingDefinition {
        id: "users.manage",
        title: "Manage Users",
        category: "users",
        icon: NerdFont::Users,
        breadcrumbs: &["Manage Users"],
        kind: SettingKind::Action {
            summary: "Create, modify, and delete user accounts.\n\nManage user groups, shells, and permissions.",
            run: users::manage_users,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "system.install_packages",
        title: "Install packages",
        category: "system",
        icon: NerdFont::Download,
        breadcrumbs: &["Install packages"],
        kind: SettingKind::Action {
            summary: "Browse and install system packages using an interactive fuzzy finder.",
            run: super::packages::run_package_installer_action,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "bluetooth.service",
        title: "Enable Bluetooth",
        category: "bluetooth",
        icon: NerdFont::Bluetooth,
        breadcrumbs: &["Enable Bluetooth"],
        kind: SettingKind::Toggle {
            key: BLUETOOTH_SERVICE_KEY,
            summary: "Turn Bluetooth on or off.\n\nWhen enabled, you can connect wireless devices like headphones, keyboards, and mice.",
            apply: Some(super::actions::apply_bluetooth_service),
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "bluetooth.manager",
        title: "Manage Devices",
        category: "bluetooth",
        icon: NerdFont::Settings,
        breadcrumbs: &["Manage Devices"],
        kind: SettingKind::Command {
            summary: "Pair new devices and manage connected Bluetooth devices.\n\nUse this to connect headphones, speakers, keyboards, mice, and other wireless devices.",
            command: CommandSpec::detached("blueman-manager", &[]),
        },
        requires_reapply: false,
        requirements: &BLUETOOTH_MANAGER_REQUIREMENTS,
    },
    SettingDefinition {
        id: "network.ip_info",
        title: "IP Address Info",
        category: "network",
        icon: NerdFont::Info,
        breadcrumbs: &["IP Address Info"],
        kind: SettingKind::Action {
            summary: "View your local and public IP addresses.\n\nUseful for troubleshooting network issues or setting up remote access.",
            run: super::network::show_ip_info,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "network.speed_test",
        title: "Internet Speed Test",
        category: "network",
        icon: NerdFont::Rocket,
        breadcrumbs: &["Internet Speed Test"],
        kind: SettingKind::Action {
            summary: "Test your internet connection speed using fast.com.\n\nMeasures download speed from Netflix servers.",
            run: super::network::launch_speed_test,
        },
        requires_reapply: false,
        requirements: &[SettingRequirement::Package(super::network::CHROMIUM_PACKAGE)],
    },
    SettingDefinition {
        id: "network.edit_connections",
        title: "Edit Connections",
        category: "network",
        icon: NerdFont::Settings,
        breadcrumbs: &["Edit Connections"],
        kind: SettingKind::Action {
            summary: "Manage WiFi, Ethernet, VPN, and other network connections.\n\nConfigure connection settings, passwords, and advanced options.",
            run: super::network::edit_connections,
        },
        requires_reapply: false,
        requirements: &[SettingRequirement::Package(super::network::NM_CONNECTION_EDITOR_PACKAGE)],
    },
    SettingDefinition {
        id: "storage.automount",
        title: "Auto-mount disks",
        category: "storage",
        icon: NerdFont::HardDrive,
        breadcrumbs: &["Auto-mount disks"],
        kind: SettingKind::Toggle {
            key: UDISKIE_AUTOMOUNT_KEY,
            summary: "Automatically mount removable drives with udiskie.",
            apply: Some(super::actions::apply_udiskie_automount),
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "storage.disks",
        title: "Disk management",
        category: "storage",
        icon: NerdFont::HardDrive,
        breadcrumbs: &["Disk management"],
        kind: SettingKind::Command {
            summary: "Launch GNOME Disks to manage drives and partitions.",
            command: CommandSpec::detached("gnome-disks", &[]),
        },
        requires_reapply: false,
        requirements: &[SettingRequirement::Package(GNOME_DISKS_PACKAGE)],
    },
    SettingDefinition {
        id: "storage.gparted",
        title: "Partition editor",
        category: "storage",
        icon: NerdFont::Partition,
        breadcrumbs: &["Partition editor"],
        kind: SettingKind::Command {
            summary: "Launch GParted for advanced partition management (requires root).",
            command: CommandSpec::detached("gparted", &[]),
        },
        requires_reapply: false,
        requirements: &[SettingRequirement::Package(GPARTED_PACKAGE)],
    },
    SettingDefinition {
        id: "system.about",
        title: "About",
        category: "system",
        icon: NerdFont::About,
        breadcrumbs: &["About"],
        kind: SettingKind::Command {
            summary: "Display system information using fastfetch.",
            command: CommandSpec::terminal("sh", &["-c", "fastfetch && read -n 1"]),
        },
        requires_reapply: false,
        requirements: &[SettingRequirement::Package(FASTFETCH_PACKAGE)],
    },
    SettingDefinition {
        id: "system.cockpit",
        title: "Systemd manager (Cockpit)",
        category: "system",
        icon: NerdFont::Server,
        breadcrumbs: &["Systemd manager"],
        kind: SettingKind::Action {
            summary: "Launch Cockpit web interface for managing systemd services, logs, and system resources.",
            run: super::actions::launch_cockpit,
        },
        requires_reapply: false,
        requirements: &[],
    },
    // Quick access settings for common applications
    SettingDefinition {
        id: "apps.browser",
        title: "Web Browser",
        category: "apps",
        icon: NerdFont::Globe,
        breadcrumbs: &["Web Browser"],
        kind: SettingKind::Action {
            summary: "Set your default web browser for opening links and HTML files.",
            run: super::defaultapps::set_default_browser,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.email",
        title: "Email Client",
        category: "apps",
        icon: NerdFont::ExternalLink,
        breadcrumbs: &["Email Client"],
        kind: SettingKind::Action {
            summary: "Set your default email client for mailto: links.",
            run: super::defaultapps::set_default_email,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.file_manager",
        title: "File Manager",
        category: "apps",
        icon: NerdFont::Folder,
        breadcrumbs: &["File Manager"],
        kind: SettingKind::Action {
            summary: "Set your default file manager for browsing folders.",
            run: super::defaultapps::set_default_file_manager,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.text_editor",
        title: "Text Editor",
        category: "apps",
        icon: NerdFont::FileText,
        breadcrumbs: &["Text Editor"],
        kind: SettingKind::Action {
            summary: "Set your default text editor for opening text files.",
            run: super::defaultapps::set_default_text_editor,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.image_viewer",
        title: "Image Viewer",
        category: "apps",
        icon: NerdFont::Image,
        breadcrumbs: &["Image Viewer"],
        kind: SettingKind::Action {
            summary: "Set your default image viewer for photos and pictures.",
            run: super::defaultapps::set_default_image_viewer,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.video_player",
        title: "Video Player",
        category: "apps",
        icon: NerdFont::Video,
        breadcrumbs: &["Video Player"],
        kind: SettingKind::Action {
            summary: "Set your default video player for movies and videos.",
            run: super::defaultapps::set_default_video_player,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.music_player",
        title: "Music Player",
        category: "apps",
        icon: NerdFont::Music,
        breadcrumbs: &["Music Player"],
        kind: SettingKind::Action {
            summary: "Set your default music player for audio files.",
            run: super::defaultapps::set_default_music_player,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.pdf_viewer",
        title: "PDF Viewer",
        category: "apps",
        icon: NerdFont::FilePdf,
        breadcrumbs: &["PDF Viewer"],
        kind: SettingKind::Action {
            summary: "Set your default PDF viewer for documents.",
            run: super::defaultapps::set_default_pdf_viewer,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.archive_manager",
        title: "Archive Manager",
        category: "apps",
        icon: NerdFont::Archive,
        breadcrumbs: &["Archive Manager"],
        kind: SettingKind::Action {
            summary: "Set your default archive manager for ZIP, TAR, and other compressed files.",
            run: super::defaultapps::set_default_archive_manager,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "apps.default",
        title: "All File Types",
        category: "apps",
        icon: NerdFont::Link,
        breadcrumbs: &["All File Types"],
        kind: SettingKind::Action {
            summary: "Advanced: Manage default applications for all file types and MIME types.",
            run: super::defaultapps::manage_default_apps,
        },
        requires_reapply: false,
        requirements: &[],
    },
    SettingDefinition {
        id: "system.upgrade",
        title: "Upgrade",
        category: "system",
        icon: NerdFont::Upgrade,
        breadcrumbs: &["Upgrade"],
        kind: SettingKind::Command {
            summary: "Upgrade all installed packages and system components using topgrade.",
            command: CommandSpec::terminal("topgrade", &[]),
        },
        requires_reapply: false,
        requirements: &[SettingRequirement::Package(TOPGRADE_PACKAGE)],
    },
    SettingDefinition {
        id: "printers.enable_services",
        title: "Printer services",
        category: "printers",
        icon: NerdFont::Printer,
        breadcrumbs: &["Printer support", "Services"],
        kind: SettingKind::Toggle {
            key: BoolSettingKey::new("printers.services", false),
            summary: "Enable CUPS printing and Avahi discovery for network printers.",
            apply: Some(super::printer::configure_printer_support),
        },
        requires_reapply: false,
        requirements: &[
            SettingRequirement::Package(printer::CUPS_PACKAGE),
            SettingRequirement::Package(printer::AVAHI_PACKAGE),
            SettingRequirement::Package(printer::CUPS_FILTERS_PACKAGE),
            SettingRequirement::Package(printer::GHOSTSCRIPT_PACKAGE),
            SettingRequirement::Package(printer::NSS_MDNS_PACKAGE),
        ],
    },
    SettingDefinition {
        id: "printers.open_manager",
        title: "Open printer manager",
        category: "printers",
        icon: NerdFont::Printer,
        breadcrumbs: &["Printer support", "Manage printers"],
        kind: SettingKind::Action {
            summary: "Launch the graphical printer setup utility.",
            run: super::printer::launch_printer_manager,
        },
        requires_reapply: false,
        requirements: &[SettingRequirement::Package(
            super::printer::SYSTEM_CONFIG_PRINTER_PACKAGE,
        )],
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

pub fn setting_by_id(id: &str) -> Option<&'static SettingDefinition> {
    SETTINGS.iter().find(|setting| setting.id == id)
}
