//! Installable package collections for settings menus
//!
//! Defines curated collections of packages that can be installed
//! from within settings menus via "Install more..." options.

use crate::common::requirements::{InstallTest, RequiredPackage};

/// Represents an installable application/package collection
#[derive(Debug, Clone)]
pub struct InstallableApp {
    /// Display name for the menu
    pub name: &'static str,
    /// Brief description
    pub description: &'static str,
    /// Packages that make up this app (may be multiple, e.g., zathura + pdf plugin)
    pub packages: &'static [RequiredPackage],
}

impl InstallableApp {
    /// Check if all packages in this app are installed
    pub fn is_installed(&self) -> bool {
        self.packages.iter().all(|pkg| pkg.is_installed())
    }

    /// Get all required packages for installation
    pub fn required_packages(&self) -> Vec<RequiredPackage> {
        self.packages.to_vec()
    }
}

// =============================================================================
// PDF Viewers
// =============================================================================

pub const OKULAR_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Okular",
    arch_package_name: Some("okular"),
    ubuntu_package_name: Some("okular"),
    tests: &[InstallTest::WhichSucceeds("okular")],
};

pub const EVINCE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Evince",
    arch_package_name: Some("evince"),
    ubuntu_package_name: Some("evince"),
    tests: &[InstallTest::WhichSucceeds("evince")],
};

pub const ZATHURA_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Zathura",
    arch_package_name: Some("zathura"),
    ubuntu_package_name: Some("zathura"),
    tests: &[InstallTest::WhichSucceeds("zathura")],
};

pub const ZATHURA_PDF_MUPDF_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Zathura PDF Plugin",
    arch_package_name: Some("zathura-pdf-mupdf"),
    ubuntu_package_name: Some("zathura-pdf-poppler"),
    tests: &[
        InstallTest::FileExists("/usr/lib/zathura/libpdf-mupdf.so"),
        InstallTest::FileExists("/usr/lib/x86_64-linux-gnu/zathura/libpdf-poppler.so"),
    ],
};

pub const MUPDF_PACKAGE: RequiredPackage = RequiredPackage {
    name: "MuPDF",
    arch_package_name: Some("mupdf"),
    ubuntu_package_name: Some("mupdf"),
    tests: &[InstallTest::WhichSucceeds("mupdf")],
};

pub const QPDFVIEW_PACKAGE: RequiredPackage = RequiredPackage {
    name: "qpdfview",
    arch_package_name: Some("qpdfview"),
    ubuntu_package_name: Some("qpdfview"),
    tests: &[InstallTest::WhichSucceeds("qpdfview")],
};

pub static PDF_VIEWERS: &[InstallableApp] = &[
    InstallableApp {
        name: "Okular",
        description: "KDE document viewer with annotations support",
        packages: &[OKULAR_PACKAGE],
    },
    InstallableApp {
        name: "Evince",
        description: "GNOME document viewer, simple and lightweight",
        packages: &[EVINCE_PACKAGE],
    },
    InstallableApp {
        name: "Zathura",
        description: "Minimal keyboard-driven PDF viewer (vim-like)",
        packages: &[ZATHURA_PACKAGE, ZATHURA_PDF_MUPDF_PACKAGE],
    },
    InstallableApp {
        name: "MuPDF",
        description: "Lightweight PDF viewer with fast rendering",
        packages: &[MUPDF_PACKAGE],
    },
    InstallableApp {
        name: "qpdfview",
        description: "Tabbed document viewer with Qt interface",
        packages: &[QPDFVIEW_PACKAGE],
    },
];

// =============================================================================
// Image Viewers
// =============================================================================

pub const IMVIEW_PACKAGE: RequiredPackage = RequiredPackage {
    name: "imv",
    arch_package_name: Some("imv"),
    ubuntu_package_name: Some("imv"),
    tests: &[InstallTest::WhichSucceeds("imv")],
};

pub const FEH_PACKAGE: RequiredPackage = RequiredPackage {
    name: "feh",
    arch_package_name: Some("feh"),
    ubuntu_package_name: Some("feh"),
    tests: &[InstallTest::WhichSucceeds("feh")],
};

pub const EOG_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Eye of GNOME",
    arch_package_name: Some("eog"),
    ubuntu_package_name: Some("eog"),
    tests: &[InstallTest::WhichSucceeds("eog")],
};

pub const GWENVIEW_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Gwenview",
    arch_package_name: Some("gwenview"),
    ubuntu_package_name: Some("gwenview"),
    tests: &[InstallTest::WhichSucceeds("gwenview")],
};

pub const SXIV_PACKAGE: RequiredPackage = RequiredPackage {
    name: "sxiv",
    arch_package_name: Some("sxiv"),
    ubuntu_package_name: Some("sxiv"),
    tests: &[InstallTest::WhichSucceeds("sxiv")],
};

pub const LOUPE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Loupe",
    arch_package_name: Some("loupe"),
    ubuntu_package_name: Some("loupe"),
    tests: &[InstallTest::WhichSucceeds("loupe")],
};

pub static IMAGE_VIEWERS: &[InstallableApp] = &[
    InstallableApp {
        name: "imv",
        description: "Simple Wayland/X11 image viewer",
        packages: &[IMVIEW_PACKAGE],
    },
    InstallableApp {
        name: "feh",
        description: "Fast, lightweight X11 image viewer",
        packages: &[FEH_PACKAGE],
    },
    InstallableApp {
        name: "Eye of GNOME",
        description: "GNOME image viewer with basic editing",
        packages: &[EOG_PACKAGE],
    },
    InstallableApp {
        name: "Gwenview",
        description: "KDE image viewer with slideshow support",
        packages: &[GWENVIEW_PACKAGE],
    },
    InstallableApp {
        name: "sxiv",
        description: "Simple X Image Viewer (keyboard-driven)",
        packages: &[SXIV_PACKAGE],
    },
    InstallableApp {
        name: "Loupe",
        description: "Modern GNOME image viewer",
        packages: &[LOUPE_PACKAGE],
    },
];

// =============================================================================
// Video Players
// =============================================================================

pub const VLC_PACKAGE: RequiredPackage = RequiredPackage {
    name: "VLC",
    arch_package_name: Some("vlc"),
    ubuntu_package_name: Some("vlc"),
    tests: &[InstallTest::WhichSucceeds("vlc")],
};

pub const MPV_PACKAGE: RequiredPackage = RequiredPackage {
    name: "mpv",
    arch_package_name: Some("mpv"),
    ubuntu_package_name: Some("mpv"),
    tests: &[InstallTest::WhichSucceeds("mpv")],
};

pub const CELLULOID_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Celluloid",
    arch_package_name: Some("celluloid"),
    ubuntu_package_name: Some("celluloid"),
    tests: &[InstallTest::WhichSucceeds("celluloid")],
};

pub const TOTEM_PACKAGE: RequiredPackage = RequiredPackage {
    name: "GNOME Videos",
    arch_package_name: Some("totem"),
    ubuntu_package_name: Some("totem"),
    tests: &[InstallTest::WhichSucceeds("totem")],
};

pub const HARUNA_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Haruna",
    arch_package_name: Some("haruna"),
    ubuntu_package_name: None,
    tests: &[InstallTest::WhichSucceeds("haruna")],
};

pub static VIDEO_PLAYERS: &[InstallableApp] = &[
    InstallableApp {
        name: "VLC",
        description: "Universal media player, plays almost anything",
        packages: &[VLC_PACKAGE],
    },
    InstallableApp {
        name: "mpv",
        description: "Minimal, scriptable video player",
        packages: &[MPV_PACKAGE],
    },
    InstallableApp {
        name: "Celluloid",
        description: "GTK frontend for mpv",
        packages: &[CELLULOID_PACKAGE],
    },
    InstallableApp {
        name: "GNOME Videos (Totem)",
        description: "Simple GNOME video player",
        packages: &[TOTEM_PACKAGE],
    },
    InstallableApp {
        name: "Haruna",
        description: "Modern Qt/KDE video player based on mpv",
        packages: &[HARUNA_PACKAGE],
    },
];

// =============================================================================
// Text Editors
// =============================================================================

pub const GEDIT_PACKAGE: RequiredPackage = RequiredPackage {
    name: "gedit",
    arch_package_name: Some("gedit"),
    ubuntu_package_name: Some("gedit"),
    tests: &[InstallTest::WhichSucceeds("gedit")],
};

pub const KATE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Kate",
    arch_package_name: Some("kate"),
    ubuntu_package_name: Some("kate"),
    tests: &[InstallTest::WhichSucceeds("kate")],
};

pub const GNOME_TEXT_EDITOR_PACKAGE: RequiredPackage = RequiredPackage {
    name: "GNOME Text Editor",
    arch_package_name: Some("gnome-text-editor"),
    ubuntu_package_name: Some("gnome-text-editor"),
    tests: &[InstallTest::WhichSucceeds("gnome-text-editor")],
};

pub const MOUSEPAD_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Mousepad",
    arch_package_name: Some("mousepad"),
    ubuntu_package_name: Some("mousepad"),
    tests: &[InstallTest::WhichSucceeds("mousepad")],
};

pub const XEDIT_PACKAGE: RequiredPackage = RequiredPackage {
    name: "xed",
    arch_package_name: Some("xed"),
    ubuntu_package_name: Some("xed"),
    tests: &[InstallTest::WhichSucceeds("xed")],
};

pub static TEXT_EDITORS: &[InstallableApp] = &[
    InstallableApp {
        name: "gedit",
        description: "Classic GNOME text editor",
        packages: &[GEDIT_PACKAGE],
    },
    InstallableApp {
        name: "Kate",
        description: "KDE advanced text editor with syntax highlighting",
        packages: &[KATE_PACKAGE],
    },
    InstallableApp {
        name: "GNOME Text Editor",
        description: "Modern GNOME text editor",
        packages: &[GNOME_TEXT_EDITOR_PACKAGE],
    },
    InstallableApp {
        name: "Mousepad",
        description: "Simple Xfce text editor",
        packages: &[MOUSEPAD_PACKAGE],
    },
    InstallableApp {
        name: "xed",
        description: "X-Apps text editor (Linux Mint)",
        packages: &[XEDIT_PACKAGE],
    },
];

// =============================================================================
// GTK Themes
// =============================================================================

pub const ADWAITA_DARK_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Adwaita (GNOME default)",
    arch_package_name: Some("gnome-themes-extra"),
    ubuntu_package_name: Some("gnome-themes-extra"),
    tests: &[InstallTest::FileExists("/usr/share/themes/Adwaita-dark")],
};

pub const ARC_THEME_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Arc Theme",
    arch_package_name: Some("arc-gtk-theme"),
    ubuntu_package_name: Some("arc-theme"),
    tests: &[InstallTest::FileExists("/usr/share/themes/Arc")],
};

pub const MATERIA_THEME_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Materia Theme",
    arch_package_name: Some("materia-gtk-theme"),
    ubuntu_package_name: Some("materia-gtk-theme"),
    tests: &[InstallTest::FileExists("/usr/share/themes/Materia")],
};

pub const BREEZE_GTK_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Breeze GTK",
    arch_package_name: Some("breeze-gtk"),
    ubuntu_package_name: Some("breeze-gtk-theme"),
    tests: &[InstallTest::FileExists("/usr/share/themes/Breeze")],
};

pub const GRUVBOX_GTK_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Gruvbox GTK",
    arch_package_name: Some("gruvbox-dark-gtk"),
    ubuntu_package_name: None,
    tests: &[InstallTest::FileExists("/usr/share/themes/Gruvbox-Dark")],
};

pub const DRACULA_GTK_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Dracula GTK",
    arch_package_name: Some("dracula-gtk-theme"),
    ubuntu_package_name: None,
    tests: &[InstallTest::FileExists("/usr/share/themes/Dracula")],
};

pub const NORDIC_THEME_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Nordic Theme",
    arch_package_name: Some("nordic-theme"),
    ubuntu_package_name: None,
    tests: &[InstallTest::FileExists("/usr/share/themes/Nordic")],
};

pub static GTK_THEMES: &[InstallableApp] = &[
    InstallableApp {
        name: "Adwaita (GNOME default)",
        description: "Standard GNOME theme with dark variant",
        packages: &[ADWAITA_DARK_PACKAGE],
    },
    InstallableApp {
        name: "Arc Theme",
        description: "Flat theme with transparent elements",
        packages: &[ARC_THEME_PACKAGE],
    },
    InstallableApp {
        name: "Materia Theme",
        description: "Material Design inspired theme",
        packages: &[MATERIA_THEME_PACKAGE],
    },
    InstallableApp {
        name: "Breeze GTK",
        description: "KDE Breeze theme for GTK apps",
        packages: &[BREEZE_GTK_PACKAGE],
    },
    InstallableApp {
        name: "Gruvbox GTK",
        description: "Retro groove color scheme theme",
        packages: &[GRUVBOX_GTK_PACKAGE],
    },
    InstallableApp {
        name: "Dracula GTK",
        description: "Dark theme based on Dracula color palette",
        packages: &[DRACULA_GTK_PACKAGE],
    },
    InstallableApp {
        name: "Nordic Theme",
        description: "Nord color palette based theme",
        packages: &[NORDIC_THEME_PACKAGE],
    },
];

// =============================================================================
// Archive Managers
// =============================================================================

pub const FILE_ROLLER_PACKAGE: RequiredPackage = RequiredPackage {
    name: "File Roller",
    arch_package_name: Some("file-roller"),
    ubuntu_package_name: Some("file-roller"),
    tests: &[InstallTest::WhichSucceeds("file-roller")],
};

pub const ARK_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Ark",
    arch_package_name: Some("ark"),
    ubuntu_package_name: Some("ark"),
    tests: &[InstallTest::WhichSucceeds("ark")],
};

pub const ENGRAMPA_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Engrampa",
    arch_package_name: Some("engrampa"),
    ubuntu_package_name: Some("engrampa"),
    tests: &[InstallTest::WhichSucceeds("engrampa")],
};

pub const XARCHIVER_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Xarchiver",
    arch_package_name: Some("xarchiver"),
    ubuntu_package_name: Some("xarchiver"),
    tests: &[InstallTest::WhichSucceeds("xarchiver")],
};

pub const LXQT_ARCHIVER_PACKAGE: RequiredPackage = RequiredPackage {
    name: "LXQt Archiver",
    arch_package_name: Some("lxqt-archiver"),
    ubuntu_package_name: Some("lxqt-archiver"),
    tests: &[InstallTest::WhichSucceeds("lxqt-archiver")],
};

pub static ARCHIVE_MANAGERS: &[InstallableApp] = &[
    InstallableApp {
        name: "File Roller",
        description: "GNOME archive manager",
        packages: &[FILE_ROLLER_PACKAGE],
    },
    InstallableApp {
        name: "Ark",
        description: "KDE archive manager",
        packages: &[ARK_PACKAGE],
    },
    InstallableApp {
        name: "Engrampa",
        description: "MATE archive manager",
        packages: &[ENGRAMPA_PACKAGE],
    },
    InstallableApp {
        name: "Xarchiver",
        description: "Lightweight GTK archive manager",
        packages: &[XARCHIVER_PACKAGE],
    },
    InstallableApp {
        name: "LXQt Archiver",
        description: "Qt-based lightweight archive manager",
        packages: &[LXQT_ARCHIVER_PACKAGE],
    },
];

// =============================================================================
// Install More Menu Helper
// =============================================================================

use crate::common::requirements::ensure_packages_batch;
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::NerdFont;
use anyhow::Result;

/// Wrapper for FZF display
#[derive(Clone)]
struct InstallableAppItem<'a> {
    app: &'a InstallableApp,
}

impl FzfSelectable for InstallableAppItem<'_> {
    fn fzf_display_text(&self) -> String {
        let status = if self.app.is_installed() {
            NerdFont::Check.to_string()
        } else {
            " ".to_string()
        };
        format!("[{}] {}", status, self.app.name)
    }

    fn fzf_key(&self) -> String {
        self.app.name.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut preview = format!("{}\n\n", self.app.description);

        preview.push_str("Packages:\n");
        for pkg in self.app.packages {
            let status = if pkg.is_installed() {
                NerdFont::Check
            } else {
                NerdFont::Circle
            };
            let pkg_name = pkg.arch_package_name.unwrap_or(pkg.name);
            preview.push_str(&format!("  {} {}\n", status, pkg_name));
        }

        if self.app.is_installed() {
            preview.push_str(&format!("\n{} Already installed", NerdFont::Check));
        } else {
            preview.push_str(&format!(
                "\n{} Not installed - select to install",
                NerdFont::Circle
            ));
        }

        FzfPreview::Text(preview)
    }
}

/// Show install more menu for a given category
/// Returns true if something was installed
pub fn show_install_more_menu(category_name: &str, apps: &[InstallableApp]) -> Result<bool> {
    let items: Vec<InstallableAppItem> =
        apps.iter().map(|app| InstallableAppItem { app }).collect();

    let header = format!(
        "Select an application to install\n[{}] = installed, [ ] = not installed",
        NerdFont::Check
    );

    let selected = FzfWrapper::builder()
        .prompt(format!("Install {}: ", category_name))
        .header(&header)
        .select(items)?;

    match selected {
        FzfResult::Selected(item) => {
            if item.app.is_installed() {
                FzfWrapper::builder()
                    .message(format!("{} is already installed.", item.app.name))
                    .title("Already Installed")
                    .show_message()?;
                return Ok(false);
            }

            let packages: Vec<_> = item.app.packages.to_vec();
            let status = ensure_packages_batch(&packages)?;
            Ok(status.is_installed())
        }
        FzfResult::MultiSelected(_) => {
            // Multi-selection not used, treat as cancelled
            Ok(false)
        }
        FzfResult::Cancelled | FzfResult::Error(_) => Ok(false),
    }
}
