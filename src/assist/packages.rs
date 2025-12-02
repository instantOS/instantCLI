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

// Package definitions - referenced directly in registry.rs
pub(super) static PLAYERCTL: RequiredPackage = pkg!("playerctl");
pub(super) static QRENCODE: RequiredPackage = pkg!("qrencode");
pub(super) static ZBAR: RequiredPackage = pkg!("zbar", "zbarimg");
pub(super) static FLAMESHOT: RequiredPackage = pkg!("flameshot");
pub(super) static MPV: RequiredPackage = pkg!("mpv");
pub(super) static TESSERACT: RequiredPackage = pkg!("tesseract");
pub(super) static BRIGHTNESSCTL: RequiredPackage = pkg!("brightnessctl");
pub(super) static ASCIIQUARIUM: RequiredPackage = pkg!("asciiquarium");
pub(super) static CMATRIX: RequiredPackage = pkg!("cmatrix");
pub(super) static SLURP: RequiredPackage = pkg!("slurp");
pub(super) static SLOP: RequiredPackage = pkg!("slop");
pub(super) static GRIM: RequiredPackage = pkg!("grim");
pub(super) static IMAGEMAGICK: RequiredPackage = pkg!("imagemagick", "import");
pub(super) static WL_CLIPBOARD: RequiredPackage = pkg!("wl-clipboard", "wl-copy");
pub(super) static XCLIP: RequiredPackage = pkg!("xclip");
pub(super) static LIBNOTIFY: RequiredPackage = RequiredPackage {
    name: "libnotify",
    arch_package_name: Some("libnotify"),
    ubuntu_package_name: Some("libnotify-bin"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "notify-send",
    )],
};

pub(super) static HYPRPICKER: RequiredPackage = pkg!("hyprpicker");
pub(super) static XCOLOR: RequiredPackage = RequiredPackage {
    name: "xcolor",
    arch_package_name: Some("xcolor"),
    ubuntu_package_name: None, // Not available in Ubuntu repos, needs cargo install
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "xcolor",
    )],
};

pub(super) static EMOTE: FlatpakPackage = FlatpakPackage {
    name: "Emote",
    app_id: "com.tomjwatson.Emote",
    tests: &[crate::common::requirements::InstallTest::CommandSucceeds {
        program: "flatpak",
        args: &["info", "com.tomjwatson.Emote"],
    }],
};
