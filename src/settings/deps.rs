//! Settings-specific package dependencies
//!
//! Dependency definitions for packages used by settings.

use crate::common::package::{Dependency, PackageDefinition, PackageManager};
use crate::common::requirements::InstallTest;

// =============================================================================
// Clipboard
// =============================================================================

pub static CLIPMENU: Dependency = Dependency {
    name: "clipmenu",
    description: Some("Clipboard manager daemon"),
    packages: &[PackageDefinition::new("clipmenu", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("clipmenud")],
};

// =============================================================================
// Storage
// =============================================================================

pub static UDISKIE: Dependency = Dependency {
    name: "udiskie",
    description: Some("Automounter for removable media"),
    packages: &[
        PackageDefinition::new("udiskie", PackageManager::Pacman),
        PackageDefinition::new("udiskie", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("udiskie")],
};

// =============================================================================
// Bluetooth
// =============================================================================

pub static BLUEZ: Dependency = Dependency {
    name: "BlueZ bluetooth daemon",
    description: Some("Linux Bluetooth protocol stack"),
    packages: &[
        PackageDefinition::new("bluez", PackageManager::Pacman),
        PackageDefinition::new("bluez", PackageManager::Apt),
    ],
    tests: &[
        InstallTest::WhichSucceeds("bluetoothd"),
        InstallTest::FileExists("/usr/lib/systemd/system/bluetooth.service"),
    ],
};

pub static BLUEZ_UTILS: Dependency = Dependency {
    name: "BlueZ utilities",
    description: Some("Bluetooth utilities (bluetoothctl)"),
    packages: &[
        PackageDefinition::new("bluez-utils", PackageManager::Pacman),
        PackageDefinition::new("bluez", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("bluetoothctl")],
};

// =============================================================================
// System
// =============================================================================

pub static COCKPIT: Dependency = Dependency {
    name: "Cockpit",
    description: Some("Web-based system administration"),
    packages: &[
        PackageDefinition::new("cockpit", PackageManager::Pacman),
        PackageDefinition::new("cockpit", PackageManager::Apt),
    ],
    tests: &[
        InstallTest::FileExists("/usr/lib/systemd/system/cockpit.socket"),
        InstallTest::WhichSucceeds("cockpit-bridge"),
    ],
};

pub static FASTFETCH: Dependency = Dependency {
    name: "fastfetch",
    description: Some("System information tool"),
    packages: &[
        PackageDefinition::new("fastfetch", PackageManager::Pacman),
        PackageDefinition::new("fastfetch", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("fastfetch")],
};

pub static PACMAN_CONTRIB: Dependency = Dependency {
    name: "pacman-contrib",
    description: Some("Pacman helper utilities"),
    packages: &[PackageDefinition::new("pacman-contrib", PackageManager::Pacman)],
    tests: &[
        InstallTest::WhichSucceeds("paccache"),
        InstallTest::WhichSucceeds("checkupdates"),
    ],
};

pub static TOPGRADE: Dependency = Dependency {
    name: "topgrade",
    description: Some("System upgrade tool"),
    packages: &[PackageDefinition::new("topgrade", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("topgrade")],
};

pub static GNOME_FIRMWARE: Dependency = Dependency {
    name: "GNOME Firmware manager",
    description: Some("Device firmware manager"),
    packages: &[
        PackageDefinition::new("gnome-firmware", PackageManager::Pacman),
        PackageDefinition::new("gnome-firmware", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("gnome-firmware")],
};

pub static CHROMIUM: Dependency = Dependency {
    name: "Chromium browser",
    description: Some("Open source web browser"),
    packages: &[
        PackageDefinition::new("chromium", PackageManager::Pacman),
        PackageDefinition::new("chromium-browser", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("chromium")],
};

// Cockpit requires both cockpit and chromium for the browser interface
pub static COCKPIT_DEPS: &[&Dependency] = &[&COCKPIT, &CHROMIUM];

// =============================================================================
// Default Apps (apps.rs)
// =============================================================================

pub static XDG_UTILS: Dependency = Dependency {
    name: "xdg-utils",
    description: Some("Desktop integration utilities"),
    packages: &[
        PackageDefinition::new("xdg-utils", PackageManager::Pacman),
        PackageDefinition::new("xdg-utils", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("xdg-open")],
};

// =============================================================================
// Network (network.rs)
// =============================================================================

pub static NM_CONNECTION_EDITOR: Dependency = Dependency {
    name: "Network connection editor",
    description: Some("NetworkManager connection editor"),
    packages: &[
        PackageDefinition::new("nm-connection-editor", PackageManager::Pacman),
        PackageDefinition::new("network-manager-gnome", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("nm-connection-editor")],
};

// =============================================================================
// Appearance (appearance.rs)
// =============================================================================

pub static YAZI: Dependency = Dependency {
    name: "Yazi file manager",
    description: Some("Terminal file manager with image preview"),
    packages: &[
        PackageDefinition::new("yazi", PackageManager::Pacman),
        PackageDefinition::new("yazi", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("yazi")],
};

pub static ZENITY: Dependency = Dependency {
    name: "Zenity dialogs",
    description: Some("GTK dialog utility"),
    packages: &[
        PackageDefinition::new("zenity", PackageManager::Pacman),
        PackageDefinition::new("zenity", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("zenity")],
};

// =============================================================================
// Printer (printer.rs)
// =============================================================================

pub static CUPS: Dependency = Dependency {
    name: "CUPS print server",
    description: Some("Common Unix Printing System"),
    packages: &[
        PackageDefinition::new("cups", PackageManager::Pacman),
        PackageDefinition::new("cups", PackageManager::Apt),
    ],
    tests: &[
        InstallTest::FileExists("/usr/bin/cupsd"),
        InstallTest::FileExists("/usr/sbin/cupsd"),
    ],
};

pub static CUPS_FILTERS: Dependency = Dependency {
    name: "cups-filters",
    description: Some("Driverless printing support"),
    packages: &[
        PackageDefinition::new("cups-filters", PackageManager::Pacman),
        PackageDefinition::new("cups-filters", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("cupsfilter")],
};

pub static GHOSTSCRIPT: Dependency = Dependency {
    name: "Ghostscript",
    description: Some("PostScript/PDF interpreter"),
    packages: &[
        PackageDefinition::new("ghostscript", PackageManager::Pacman),
        PackageDefinition::new("ghostscript", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("gs")],
};

pub static AVAHI: Dependency = Dependency {
    name: "Avahi",
    description: Some("mDNS/DNS-SD discovery daemon"),
    packages: &[
        PackageDefinition::new("avahi", PackageManager::Pacman),
        PackageDefinition::new("avahi-daemon", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("avahi-daemon")],
};

pub static SYSTEM_CONFIG_PRINTER: Dependency = Dependency {
    name: "Printer configuration utility",
    description: Some("CUPS printer configuration tool"),
    packages: &[
        PackageDefinition::new("system-config-printer", PackageManager::Pacman),
        PackageDefinition::new("system-config-printer", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("system-config-printer")],
};

pub static NSS_MDNS: Dependency = Dependency {
    name: "nss-mdns",
    description: Some("mDNS name resolution"),
    packages: &[
        PackageDefinition::new("nss-mdns", PackageManager::Pacman),
        PackageDefinition::new("libnss-mdns", PackageManager::Apt),
    ],
    tests: &[
        InstallTest::FileExists("/usr/lib/libnss_mdns.so.2"),
        InstallTest::FileExists("/usr/lib/x86_64-linux-gnu/libnss_mdns.so.2"),
    ],
};

/// All printer-related dependencies for installation
pub static PRINTER_DEPS: &[&Dependency] = &[
    &CUPS,
    &CUPS_FILTERS,
    &GHOSTSCRIPT,
    &AVAHI,
    &SYSTEM_CONFIG_PRINTER,
    &NSS_MDNS,
];

// =============================================================================
// Desktop (desktop.rs)
// =============================================================================

pub static PIPER: Dependency = Dependency {
    name: "Piper mouse configurator",
    description: Some("Gaming mouse configuration tool"),
    packages: &[
        PackageDefinition::new("piper", PackageManager::Pacman),
        PackageDefinition::new("piper", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("piper")],
};

pub static BLUEMAN: Dependency = Dependency {
    name: "Blueman",
    description: Some("GTK Bluetooth manager"),
    packages: &[
        PackageDefinition::new("blueman", PackageManager::Pacman),
        PackageDefinition::new("blueman", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("blueman-manager")],
};

// =============================================================================
// Storage (storage.rs)
// =============================================================================

pub static GNOME_DISKS: Dependency = Dependency {
    name: "GNOME Disks",
    description: Some("Disk management utility"),
    packages: &[
        PackageDefinition::new("gnome-disk-utility", PackageManager::Pacman),
        PackageDefinition::new("gnome-disk-utility", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("gnome-disks")],
};

pub static GPARTED: Dependency = Dependency {
    name: "GParted",
    description: Some("Partition editor"),
    packages: &[
        PackageDefinition::new("gparted", PackageManager::Pacman),
        PackageDefinition::new("gparted", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("gparted")],
};

// =============================================================================
// Audio (wiremix.rs)
// =============================================================================

pub static WIREMIX: Dependency = Dependency {
    name: "Wiremix",
    description: Some("PipeWire TUI audio mixer"),
    packages: &[PackageDefinition::new("wiremix", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("wiremix")],
};
