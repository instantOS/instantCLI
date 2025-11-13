use crate::common::requirements::RequiredPackage;

pub static PLAYERCTL_PACKAGE: RequiredPackage = RequiredPackage {
    name: "playerctl",
    arch_package_name: Some("playerctl"),
    ubuntu_package_name: Some("playerctl"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "playerctl",
    )],
};

pub static QRENCODE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "qrencode",
    arch_package_name: Some("qrencode"),
    ubuntu_package_name: Some("qrencode"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "qrencode",
    )],
};

pub static FLAMESHOT_PACKAGE: RequiredPackage = RequiredPackage {
    name: "flameshot",
    arch_package_name: Some("flameshot"),
    ubuntu_package_name: Some("flameshot"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
        "flameshot",
    )],
};

// Area selection tools (shared by all screenshot functions)
pub static AREA_SELECTION_PACKAGES: &[RequiredPackage] = &[
    RequiredPackage {
        name: "slurp",
        arch_package_name: Some("slurp"),
        ubuntu_package_name: Some("slurp"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "slurp",
        )],
    },
    RequiredPackage {
        name: "slop",
        arch_package_name: Some("slop"),
        ubuntu_package_name: Some("slop"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "slop",
        )],
    },
];

// Screenshot capture tools
pub static SCREENSHOT_CAPTURE_PACKAGES: &[RequiredPackage] = &[
    RequiredPackage {
        name: "grim",
        arch_package_name: Some("grim"),
        ubuntu_package_name: Some("grim"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "grim",
        )],
    },
    RequiredPackage {
        name: "imagemagick",
        arch_package_name: Some("imagemagick"),
        ubuntu_package_name: Some("imagemagick"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "import",
        )],
    },
];

// Clipboard tools
pub static CLIPBOARD_PACKAGES: &[RequiredPackage] = &[
    RequiredPackage {
        name: "wl-clipboard",
        arch_package_name: Some("wl-clipboard"),
        ubuntu_package_name: Some("wl-clipboard"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "wl-copy",
        )],
    },
    RequiredPackage {
        name: "xclip",
        arch_package_name: Some("xclip"),
        ubuntu_package_name: Some("xclip"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "xclip",
        )],
    },
];

// Additional tools for Imgur upload
pub static IMGUR_UPLOAD_PACKAGES: &[RequiredPackage] = &[
    RequiredPackage {
        name: "curl",
        arch_package_name: Some("curl"),
        ubuntu_package_name: Some("curl"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "curl",
        )],
    },
    RequiredPackage {
        name: "jq",
        arch_package_name: Some("jq"),
        ubuntu_package_name: Some("jq"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "jq",
        )],
    },
];

// Notification tools
pub static NOTIFICATION_PACKAGES: &[RequiredPackage] = &[
    RequiredPackage {
        name: "libnotify",
        arch_package_name: Some("libnotify"),
        ubuntu_package_name: Some("libnotify-bin"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "notify-send",
        )],
    },
];

// Convenience arrays for backwards compatibility
pub static SCREENSHOT_CLIPBOARD_PACKAGES: &[RequiredPackage] = &[
    // Area selection tools
    RequiredPackage {
        name: "slurp",
        arch_package_name: Some("slurp"),
        ubuntu_package_name: Some("slurp"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "slurp",
        )],
    },
    RequiredPackage {
        name: "slop",
        arch_package_name: Some("slop"),
        ubuntu_package_name: Some("slop"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "slop",
        )],
    },
    // Screenshot capture tools
    RequiredPackage {
        name: "grim",
        arch_package_name: Some("grim"),
        ubuntu_package_name: Some("grim"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "grim",
        )],
    },
    RequiredPackage {
        name: "imagemagick",
        arch_package_name: Some("imagemagick"),
        ubuntu_package_name: Some("imagemagick"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "import",
        )],
    },
    // Clipboard tools
    RequiredPackage {
        name: "wl-clipboard",
        arch_package_name: Some("wl-clipboard"),
        ubuntu_package_name: Some("wl-clipboard"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "wl-copy",
        )],
    },
    RequiredPackage {
        name: "xclip",
        arch_package_name: Some("xclip"),
        ubuntu_package_name: Some("xclip"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "xclip",
        )],
    },
];

pub static SCREENSHOT_IMGUR_PACKAGES: &[RequiredPackage] = &[
    // Area selection tools
    RequiredPackage {
        name: "slurp",
        arch_package_name: Some("slurp"),
        ubuntu_package_name: Some("slurp"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "slurp",
        )],
    },
    RequiredPackage {
        name: "slop",
        arch_package_name: Some("slop"),
        ubuntu_package_name: Some("slop"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "slop",
        )],
    },
    // Screenshot capture tools
    RequiredPackage {
        name: "grim",
        arch_package_name: Some("grim"),
        ubuntu_package_name: Some("grim"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "grim",
        )],
    },
    RequiredPackage {
        name: "imagemagick",
        arch_package_name: Some("imagemagick"),
        ubuntu_package_name: Some("imagemagick"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "import",
        )],
    },
    // Clipboard tools
    RequiredPackage {
        name: "wl-clipboard",
        arch_package_name: Some("wl-clipboard"),
        ubuntu_package_name: Some("wl-clipboard"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "wl-copy",
        )],
    },
    RequiredPackage {
        name: "xclip",
        arch_package_name: Some("xclip"),
        ubuntu_package_name: Some("xclip"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "xclip",
        )],
    },
    // Imgur upload tools
    RequiredPackage {
        name: "curl",
        arch_package_name: Some("curl"),
        ubuntu_package_name: Some("curl"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "curl",
        )],
    },
    RequiredPackage {
        name: "jq",
        arch_package_name: Some("jq"),
        ubuntu_package_name: Some("jq"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "jq",
        )],
    },
    // Notification tools
    RequiredPackage {
        name: "libnotify",
        arch_package_name: Some("libnotify"),
        ubuntu_package_name: Some("libnotify-bin"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds(
            "notify-send",
        )],
    },
];
