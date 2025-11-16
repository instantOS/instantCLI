use crate::common::requirements::{FlatpakPackage, RequiredPackage};

macro_rules! pkg {
    ($name:expr) => {
        RequiredPackage {
            name: $name,
            arch_package_name: Some($name),
            ubuntu_package_name: Some($name),
            tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
                $name,
            )],
        }
    };
    ($name:expr, $bin:expr) => {
        RequiredPackage {
            name: $name,
            arch_package_name: Some($name),
            ubuntu_package_name: Some($name),
            tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
                $bin,
            )],
        }
    };
    ($name:expr, $arch:expr, $ubuntu:expr, $bin:expr) => {
        RequiredPackage {
            name: $name,
            arch_package_name: Some($arch),
            ubuntu_package_name: Some($ubuntu),
            tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
                $bin,
            )],
        }
    };
}

// Single packages
pub static PLAYERCTL: RequiredPackage = pkg!("playerctl");
pub static QRENCODE: RequiredPackage = pkg!("qrencode");
pub static ZBAR: RequiredPackage = pkg!("zbar", "zbarimg");
pub static FLAMESHOT: RequiredPackage = pkg!("flameshot");
pub static MPV: RequiredPackage = pkg!("mpv");
pub static TESSERACT: RequiredPackage = pkg!("tesseract");
pub static BRIGHTNESSCTL: RequiredPackage = pkg!("brightnessctl");
pub static ASCIICAQUARIUM: RequiredPackage = pkg!("asciiquarium");
pub static CMATRIX: RequiredPackage = pkg!("cmatrix");

// Flatpak packages
pub static EMOTE: FlatpakPackage = FlatpakPackage {
    name: "Emote",
    app_id: "com.tomjwatson.Emote",
    tests: &[crate::common::requirements::InstallTest::CommandSucceeds {
        program: "flatpak",
        args: &["info", "com.tomjwatson.Emote"],
    }],
};

// Screenshot tools
static SLURP: RequiredPackage = pkg!("slurp");
static SLOP: RequiredPackage = pkg!("slop");
static GRIM: RequiredPackage = pkg!("grim");
static IMAGEMAGICK: RequiredPackage = pkg!("imagemagick", "import");
static WL_CLIPBOARD: RequiredPackage = pkg!("wl-clipboard", "wl-copy");
static XCLIP: RequiredPackage = pkg!("xclip");
static LIBNOTIFY: RequiredPackage = RequiredPackage {
    name: "libnotify",
    arch_package_name: Some("libnotify"),
    ubuntu_package_name: Some("libnotify-bin"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "notify-send",
    )],
};

// Composed package sets
pub static SCREENSHOT_FULLSCREEN_PACKAGES: &[RequiredPackage] = &[GRIM, IMAGEMAGICK];

pub static SCREENSHOT_CLIPBOARD_PACKAGES: &[RequiredPackage] =
    &[SLURP, SLOP, GRIM, IMAGEMAGICK, WL_CLIPBOARD, XCLIP];

pub static SCREENSHOT_IMGUR_PACKAGES: &[RequiredPackage] = &[
    SLURP,
    SLOP,
    GRIM,
    IMAGEMAGICK,
    WL_CLIPBOARD,
    XCLIP,
    LIBNOTIFY,
];

pub static SCREENSHOT_OCR_PACKAGES: &[RequiredPackage] = &[
    SLURP,
    SLOP,
    GRIM,
    IMAGEMAGICK,
    WL_CLIPBOARD,
    XCLIP,
    TESSERACT,
    LIBNOTIFY,
];

pub static QR_SCAN_PACKAGES: &[RequiredPackage] = &[
    SLURP,
    SLOP,
    GRIM,
    IMAGEMAGICK,
    ZBAR,
    LIBNOTIFY,
];
