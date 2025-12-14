//! Package dependencies for assist actions.
//!
//! These are the new-style `Dependency` definitions using the unified
//! package management system.

use crate::common::package::{Dependency, PackageDefinition, PackageManager};
use crate::common::requirements::InstallTest;

// =============================================================================
// Package Definitions using new Dependency format
// =============================================================================

/// Playerctl - MPRIS media player controller
pub static PLAYERCTL: Dependency = Dependency {
    name: "playerctl",
    description: Some("MPRIS media player controller"),
    packages: &[
        PackageDefinition::new("playerctl", PackageManager::Pacman),
        PackageDefinition::new("playerctl", PackageManager::Apt),
        PackageDefinition::new("playerctl", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("playerctl")],
};

/// QR code encoder
pub static QRENCODE: Dependency = Dependency {
    name: "qrencode",
    description: Some("QR code encoder"),
    packages: &[
        PackageDefinition::new("qrencode", PackageManager::Pacman),
        PackageDefinition::new("qrencode", PackageManager::Apt),
        PackageDefinition::new("qrencode", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("qrencode")],
};

/// ZBar - barcode/QR code reader
pub static ZBAR: Dependency = Dependency {
    name: "zbar",
    description: Some("Barcode and QR code reader"),
    packages: &[
        PackageDefinition::new("zbar", PackageManager::Pacman),
        PackageDefinition::new("zbar-tools", PackageManager::Apt),
        PackageDefinition::new("zbar", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("zbarimg")],
};

/// Flameshot - screenshot tool
pub static FLAMESHOT: Dependency = Dependency {
    name: "flameshot",
    description: Some("Powerful screenshot tool"),
    packages: &[
        PackageDefinition::new("flameshot", PackageManager::Pacman),
        PackageDefinition::new("flameshot", PackageManager::Apt),
        PackageDefinition::new("flameshot", PackageManager::Dnf),
        PackageDefinition::new("org.flameshot.Flameshot", PackageManager::Flatpak),
    ],
    tests: &[InstallTest::WhichSucceeds("flameshot")],
};

/// instantpass - password manager (Arch only)
pub static INSTANTPASS: Dependency = Dependency {
    name: "instantpass",
    description: Some("Instant password manager"),
    packages: &[
        PackageDefinition::new("instantpass", PackageManager::Pacman),
        PackageDefinition::new("instantpass", PackageManager::Aur),
    ],
    tests: &[InstallTest::WhichSucceeds("instantpass")],
};

/// mpv - minimal video player
pub static MPV: Dependency = Dependency {
    name: "mpv",
    description: Some("Minimal, scriptable video player"),
    packages: &[
        PackageDefinition::new("mpv", PackageManager::Pacman),
        PackageDefinition::new("mpv", PackageManager::Apt),
        PackageDefinition::new("mpv", PackageManager::Dnf),
        PackageDefinition::new("io.mpv.Mpv", PackageManager::Flatpak),
    ],
    tests: &[InstallTest::WhichSucceeds("mpv")],
};

/// Tesseract OCR
pub static TESSERACT: Dependency = Dependency {
    name: "tesseract",
    description: Some("OCR engine"),
    packages: &[
        PackageDefinition::new("tesseract", PackageManager::Pacman),
        PackageDefinition::new("tesseract-ocr", PackageManager::Apt),
        PackageDefinition::new("tesseract", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("tesseract")],
};

/// Brightness control
pub static BRIGHTNESSCTL: Dependency = Dependency {
    name: "brightnessctl",
    description: Some("Brightness control"),
    packages: &[
        PackageDefinition::new("brightnessctl", PackageManager::Pacman),
        PackageDefinition::new("brightnessctl", PackageManager::Apt),
        PackageDefinition::new("brightnessctl", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("brightnessctl")],
};

/// ASCII aquarium animation
pub static ASCIIQUARIUM: Dependency = Dependency {
    name: "asciiquarium",
    description: Some("ASCII art aquarium animation"),
    packages: &[
        PackageDefinition::new("asciiquarium", PackageManager::Pacman),
        PackageDefinition::new("asciiquarium", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("asciiquarium")],
};

/// Matrix animation
pub static CMATRIX: Dependency = Dependency {
    name: "cmatrix",
    description: Some("Matrix-style terminal animation"),
    packages: &[
        PackageDefinition::new("cmatrix", PackageManager::Pacman),
        PackageDefinition::new("cmatrix", PackageManager::Apt),
        PackageDefinition::new("cmatrix", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("cmatrix")],
};

/// Slurp - Wayland region selector
pub static SLURP: Dependency = Dependency {
    name: "slurp",
    description: Some("Wayland region selector"),
    packages: &[
        PackageDefinition::new("slurp", PackageManager::Pacman),
        PackageDefinition::new("slurp", PackageManager::Apt),
        PackageDefinition::new("slurp", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("slurp")],
};

/// Slop - X11 region selector
pub static SLOP: Dependency = Dependency {
    name: "slop",
    description: Some("X11 region selector"),
    packages: &[
        PackageDefinition::new("slop", PackageManager::Pacman),
        PackageDefinition::new("slop", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("slop")],
};

/// Grim - Wayland screenshot tool
pub static GRIM: Dependency = Dependency {
    name: "grim",
    description: Some("Wayland screenshot tool"),
    packages: &[
        PackageDefinition::new("grim", PackageManager::Pacman),
        PackageDefinition::new("grim", PackageManager::Apt),
        PackageDefinition::new("grim", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("grim")],
};

/// ImageMagick - image manipulation
pub static IMAGEMAGICK: Dependency = Dependency {
    name: "imagemagick",
    description: Some("Image manipulation toolkit"),
    packages: &[
        PackageDefinition::new("imagemagick", PackageManager::Pacman),
        PackageDefinition::new("imagemagick", PackageManager::Apt),
        PackageDefinition::new("ImageMagick", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("import")],
};

/// Wayland clipboard utilities
pub static WL_CLIPBOARD: Dependency = Dependency {
    name: "wl-clipboard",
    description: Some("Wayland clipboard utilities"),
    packages: &[
        PackageDefinition::new("wl-clipboard", PackageManager::Pacman),
        PackageDefinition::new("wl-clipboard", PackageManager::Apt),
        PackageDefinition::new("wl-clipboard", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("wl-copy")],
};

/// X11 clipboard utility
pub static XCLIP: Dependency = Dependency {
    name: "xclip",
    description: Some("X11 clipboard utility"),
    packages: &[
        PackageDefinition::new("xclip", PackageManager::Pacman),
        PackageDefinition::new("xclip", PackageManager::Apt),
        PackageDefinition::new("xclip", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("xclip")],
};

/// libnotify - notification library
pub static LIBNOTIFY: Dependency = Dependency {
    name: "libnotify",
    description: Some("Desktop notification library"),
    packages: &[
        PackageDefinition::new("libnotify", PackageManager::Pacman),
        PackageDefinition::new("libnotify-bin", PackageManager::Apt),
        PackageDefinition::new("libnotify", PackageManager::Dnf),
    ],
    tests: &[InstallTest::WhichSucceeds("notify-send")],
};

/// Hyprpicker - Hyprland color picker
pub static HYPRPICKER: Dependency = Dependency {
    name: "hyprpicker",
    description: Some("Hyprland color picker"),
    packages: &[
        PackageDefinition::new("hyprpicker", PackageManager::Pacman),
        PackageDefinition::new("hyprpicker", PackageManager::Aur),
    ],
    tests: &[InstallTest::WhichSucceeds("hyprpicker")],
};

/// xcolor - X11 color picker (with Cargo fallback)
pub static XCOLOR: Dependency = Dependency {
    name: "xcolor",
    description: Some("X11 color picker"),
    packages: &[
        PackageDefinition::new("xcolor", PackageManager::Pacman),
        PackageDefinition::new("xcolor", PackageManager::Cargo),
    ],
    tests: &[InstallTest::WhichSucceeds("xcolor")],
};

/// Emote - emoji picker (Flatpak only)
pub static EMOTE: Dependency = Dependency {
    name: "Emote",
    description: Some("Emoji picker for Linux"),
    packages: &[PackageDefinition::new(
        "com.tomjwatson.Emote",
        PackageManager::Flatpak,
    )],
    tests: &[InstallTest::CommandSucceeds {
        program: "flatpak",
        args: &["info", "com.tomjwatson.Emote"],
    }],
};
