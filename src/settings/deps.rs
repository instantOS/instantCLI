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
