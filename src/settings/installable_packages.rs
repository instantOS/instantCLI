//! Installable package collections for settings menus
//!
//! Defines curated collections of packages that can be installed
//! from within settings menus via "Install more..." options.

use crate::common::package::{
    Dependency, InstallResult, PackageDefinition, PackageManager, ensure_all,
};
use crate::common::requirements::InstallTest;
use crate::dep;

/// Represents an installable application/package collection
#[derive(Debug, Clone)]
pub struct InstallableApp {
    /// Display name for the menu
    pub name: &'static str,
    /// Brief description
    pub description: &'static str,
    /// Dependencies that make up this app (may be multiple, e.g., zathura + pdf plugin)
    pub deps: &'static [&'static Dependency],
}

impl InstallableApp {
    /// Check if all dependencies in this app are installed
    pub fn is_installed(&self) -> bool {
        self.deps.iter().all(|dep| dep.is_installed())
    }

    /// Get all dependencies for installation
    pub fn dependencies(&self) -> Vec<&'static Dependency> {
        self.deps.to_vec()
    }
}

// =============================================================================
// PDF Viewers
// =============================================================================

dep!(OKULAR, "Okular", "okular");
dep!(EVINCE, "Evince", "evince");
dep!(ZATHURA, "Zathura", "zathura");

pub static ZATHURA_PDF_MUPDF: Dependency = Dependency {
    name: "Zathura PDF Plugin",
    description: None,
    packages: &[
        PackageDefinition::new("zathura-pdf-mupdf", PackageManager::Pacman),
        PackageDefinition::new("zathura-pdf-poppler", PackageManager::Apt),
    ],
    tests: &[
        InstallTest::FileExists("/usr/lib/zathura/libpdf-mupdf.so"),
        InstallTest::FileExists("/usr/lib/x86_64-linux-gnu/zathura/libpdf-poppler.so"),
    ],
};

dep!(MUPDF, "MuPDF", "mupdf");
dep!(QPDFVIEW, "qpdfview", "qpdfview");

pub static PDF_VIEWERS: &[InstallableApp] = &[
    InstallableApp {
        name: "Okular",
        description: "KDE document viewer with annotations support",
        deps: &[&OKULAR],
    },
    InstallableApp {
        name: "Evince",
        description: "GNOME document viewer, simple and lightweight",
        deps: &[&EVINCE],
    },
    InstallableApp {
        name: "Zathura",
        description: "Minimal keyboard-driven PDF viewer (vim-like)",
        deps: &[&ZATHURA, &ZATHURA_PDF_MUPDF],
    },
    InstallableApp {
        name: "MuPDF",
        description: "Lightweight PDF viewer with fast rendering",
        deps: &[&MUPDF],
    },
    InstallableApp {
        name: "qpdfview",
        description: "Tabbed document viewer with Qt interface",
        deps: &[&QPDFVIEW],
    },
];

// =============================================================================
// Image Viewers
// =============================================================================

dep!(IMV, "imv", "imv");
dep!(FEH, "feh", "feh");
dep!(EOG, "Eye of GNOME", "eog");
dep!(GWENVIEW, "Gwenview", "gwenview");
dep!(SXIV, "sxiv", "sxiv");
dep!(LOUPE, "Loupe", "loupe");

pub static IMAGE_VIEWERS: &[InstallableApp] = &[
    InstallableApp {
        name: "imv",
        description: "Simple Wayland/X11 image viewer",
        deps: &[&IMV],
    },
    InstallableApp {
        name: "feh",
        description: "Fast, lightweight X11 image viewer",
        deps: &[&FEH],
    },
    InstallableApp {
        name: "Eye of GNOME",
        description: "GNOME image viewer with basic editing",
        deps: &[&EOG],
    },
    InstallableApp {
        name: "Gwenview",
        description: "KDE image viewer with slideshow support",
        deps: &[&GWENVIEW],
    },
    InstallableApp {
        name: "sxiv",
        description: "Simple X Image Viewer (keyboard-driven)",
        deps: &[&SXIV],
    },
    InstallableApp {
        name: "Loupe",
        description: "Modern GNOME image viewer",
        deps: &[&LOUPE],
    },
];

// =============================================================================
// Video Players
// =============================================================================

dep!(VLC, "VLC", "vlc");
dep!(MPV, "mpv", "mpv");
dep!(CELLULOID, "Celluloid", "celluloid");
dep!(TOTEM, "GNOME Videos", "totem");

pub static HARUNA: Dependency = Dependency {
    name: "Haruna",
    description: Some("Modern Qt/KDE video player based on mpv"),
    packages: &[PackageDefinition::new("haruna", PackageManager::Pacman)],
    tests: &[InstallTest::WhichSucceeds("haruna")],
};

pub static VIDEO_PLAYERS: &[InstallableApp] = &[
    InstallableApp {
        name: "VLC",
        description: "Universal media player, plays almost anything",
        deps: &[&VLC],
    },
    InstallableApp {
        name: "mpv",
        description: "Minimal, scriptable video player",
        deps: &[&MPV],
    },
    InstallableApp {
        name: "Celluloid",
        description: "GTK frontend for mpv",
        deps: &[&CELLULOID],
    },
    InstallableApp {
        name: "GNOME Videos (Totem)",
        description: "Simple GNOME video player",
        deps: &[&TOTEM],
    },
    InstallableApp {
        name: "Haruna",
        description: "Modern Qt/KDE video player based on mpv",
        deps: &[&HARUNA],
    },
];

// =============================================================================
// Text Editors
// =============================================================================

dep!(GEDIT, "gedit", "gedit");
dep!(KATE, "Kate", "kate");
dep!(GNOME_TEXT_EDITOR, "GNOME Text Editor", "gnome-text-editor");
dep!(MOUSEPAD, "Mousepad", "mousepad");
dep!(XED, "xed", "xed");
dep!(PLUMA, "pluma", "pluma");

pub static TEXT_EDITORS: &[InstallableApp] = &[
    InstallableApp {
        name: "gedit",
        description: "Classic GNOME text editor",
        deps: &[&GEDIT],
    },
    InstallableApp {
        name: "Kate",
        description: "KDE advanced text editor with syntax highlighting",
        deps: &[&KATE],
    },
    InstallableApp {
        name: "GNOME Text Editor",
        description: "Modern GNOME text editor",
        deps: &[&GNOME_TEXT_EDITOR],
    },
    InstallableApp {
        name: "Mousepad",
        description: "Simple Xfce text editor",
        deps: &[&MOUSEPAD],
    },
    InstallableApp {
        name: "xed",
        description: "X-Apps text editor (Linux Mint)",
        deps: &[&XED],
    },
    InstallableApp {
        name: "pluma",
        description: "MATE desktop text editor",
        deps: &[&PLUMA],
    },
];

// =============================================================================
// GTK Themes
// =============================================================================

pub static ADWAITA_DARK: Dependency = Dependency {
    name: "Adwaita (GNOME default)",
    description: Some("Standard GNOME theme with dark variant"),
    packages: &[
        PackageDefinition::new("gnome-themes-extra", PackageManager::Pacman),
        PackageDefinition::new("gnome-themes-extra", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/themes/Adwaita-dark")],
};

pub static ADW_GTK_THEME: Dependency = Dependency {
    name: "Adwaita-like GTK Theme",
    description: Some("Modern Adwaita-like theme for GTK applications"),
    packages: &[
        PackageDefinition::new("adw-gtk-theme", PackageManager::Pacman),
        PackageDefinition::new("adw-gtk-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/themes/adw-gtk3-dark")],
};

pub static MATERIA_THEME: Dependency = Dependency {
    name: "Materia Theme",
    description: Some("Material Design inspired theme"),
    packages: &[
        PackageDefinition::new("materia-gtk-theme", PackageManager::Pacman),
        PackageDefinition::new("materia-gtk-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/themes/Materia")],
};

pub static BREEZE_GTK: Dependency = Dependency {
    name: "Breeze GTK",
    description: Some("KDE Breeze theme for GTK apps"),
    packages: &[
        PackageDefinition::new("breeze-gtk", PackageManager::Pacman),
        PackageDefinition::new("breeze-gtk-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/themes/Breeze")],
};

pub static ELEMENTARY_THEME: Dependency = Dependency {
    name: "elementary OS Theme",
    description: Some("Clean and modern theme from elementary OS"),
    packages: &[
        PackageDefinition::new("gtk-theme-elementary", PackageManager::Pacman),
        PackageDefinition::new("gtk-theme-elementary", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists(
        "/usr/share/themes/io.elementary.stylesheet",
    )],
};

pub static ORCHIS_THEME: Dependency = Dependency {
    name: "Orchis Theme",
    description: Some("Modern Material Design theme with rounded corners"),
    packages: &[
        PackageDefinition::new("orchis-theme", PackageManager::Pacman),
        PackageDefinition::new("orchis-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/themes/Orchis-dark")],
};

pub static ADAPTA_THEME: Dependency = Dependency {
    name: "Adapta Theme",
    description: Some("Adaptive GTK theme based on Material Design"),
    packages: &[
        PackageDefinition::new("adapta-gtk-theme", PackageManager::Pacman),
        PackageDefinition::new("adapta-gtk-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/themes/Adapta")],
};

pub static POP_THEME: Dependency = Dependency {
    name: "Pop!_OS Theme",
    description: Some("Elegant dark theme from Pop!_OS"),
    packages: &[
        PackageDefinition::new("pop-gtk-theme", PackageManager::Pacman),
        PackageDefinition::new("pop-gtk-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/themes/Pop")],
};

pub static GTK_THEMES: &[InstallableApp] = &[
    InstallableApp {
        name: "Adwaita (GNOME default)",
        description: "Standard GNOME theme with dark variant",
        deps: &[&ADWAITA_DARK],
    },
    InstallableApp {
        name: "Adwaita-like GTK Theme",
        description: "Modern Adwaita-like theme for GTK applications",
        deps: &[&ADW_GTK_THEME],
    },
    InstallableApp {
        name: "Materia Theme",
        description: "Material Design inspired theme",
        deps: &[&MATERIA_THEME],
    },
    InstallableApp {
        name: "Breeze GTK",
        description: "KDE Breeze theme for GTK apps",
        deps: &[&BREEZE_GTK],
    },
    InstallableApp {
        name: "elementary OS Theme",
        description: "Clean and modern theme from elementary OS",
        deps: &[&ELEMENTARY_THEME],
    },
    InstallableApp {
        name: "Orchis Theme",
        description: "Modern Material Design theme with rounded corners",
        deps: &[&ORCHIS_THEME],
    },
    InstallableApp {
        name: "Adapta Theme",
        description: "Adaptive GTK theme based on Material Design",
        deps: &[&ADAPTA_THEME],
    },
    InstallableApp {
        name: "Pop!_OS Theme",
        description: "Elegant dark theme from Pop!_OS",
        deps: &[&POP_THEME],
    },
];

// =============================================================================
// GTK Icon Themes
// =============================================================================

pub static ADWAITA_ICON_THEME: Dependency = Dependency {
    name: "Adwaita Icons",
    description: Some("Default GNOME icon theme with modern design"),
    packages: &[
        PackageDefinition::new("adwaita-icon-theme", PackageManager::Pacman),
        PackageDefinition::new("adwaita-icon-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/icons/Adwaita")],
};

pub static PAPIRUS_ICON_THEME: Dependency = Dependency {
    name: "Papirus Icons",
    description: Some("Modern paper-like icon theme with shadows"),
    packages: &[
        PackageDefinition::new("papirus-icon-theme", PackageManager::Pacman),
        PackageDefinition::new("papirus-icon-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/icons/Papirus")],
};

pub static TELA_CIRCLE_ICON_THEME: Dependency = Dependency {
    name: "Tela Circle Icons",
    description: Some("Flat colorful circle-shaped icon theme"),
    packages: &[
        PackageDefinition::new("tela-circle-icon-theme", PackageManager::Pacman),
        PackageDefinition::new("tela-circle-icon-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/icons/Tela-circle")],
};

pub static OBSIDIAN_ICON_THEME: Dependency = Dependency {
    name: "Obsidian Icons",
    description: Some("Dark icon theme with colorful accents"),
    packages: &[
        PackageDefinition::new("obsidian-icon-theme", PackageManager::Pacman),
        PackageDefinition::new("obsidian-icon-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/icons/Obsidian")],
};

pub static COSMIC_ICON_THEME: Dependency = Dependency {
    name: "COSMIC Icons",
    description: Some("Modern icon theme from COSMIC desktop"),
    packages: &[
        PackageDefinition::new("cosmic-icon-theme", PackageManager::Pacman),
        PackageDefinition::new("cosmic-icon-theme", PackageManager::Apt),
    ],
    tests: &[InstallTest::FileExists("/usr/share/icons/COSMIC")],
};

pub static GTK_ICON_THEMES: &[InstallableApp] = &[
    InstallableApp {
        name: "Adwaita Icons",
        description: "Default GNOME icon theme with modern design",
        deps: &[&ADWAITA_ICON_THEME],
    },
    InstallableApp {
        name: "Papirus Icons",
        description: "Modern paper-like icon theme with shadows",
        deps: &[&PAPIRUS_ICON_THEME],
    },
    InstallableApp {
        name: "Tela Circle Icons",
        description: "Flat colorful circle-shaped icon theme",
        deps: &[&TELA_CIRCLE_ICON_THEME],
    },
    InstallableApp {
        name: "Obsidian Icons",
        description: "Dark icon theme with colorful accents",
        deps: &[&OBSIDIAN_ICON_THEME],
    },
    InstallableApp {
        name: "COSMIC Icons",
        description: "Modern icon theme from COSMIC desktop",
        deps: &[&COSMIC_ICON_THEME],
    },
];

// =============================================================================
// File Managers
// =============================================================================

dep!(NAUTILUS, "Nautilus", "nautilus");
dep!(DOLPHIN, "Dolphin", "dolphin");
dep!(THUNAR, "Thunar", "thunar");
dep!(PCMANFM, "PCManFM", "pcmanfm");
dep!(PCMANFM_QT, "PCManFM-Qt", "pcmanfm-qt");
dep!(RANGER, "ranger", "ranger");
dep!(LF, "lf", "lf");

pub static FILE_MANAGERS: &[InstallableApp] = &[
    InstallableApp {
        name: "Nautilus",
        description: "GNOME file manager with advanced features",
        deps: &[&NAUTILUS],
    },
    InstallableApp {
        name: "Dolphin",
        description: "KDE file manager with panels and split views",
        deps: &[&DOLPHIN],
    },
    InstallableApp {
        name: "Thunar",
        description: "Xfce file manager, fast and lightweight",
        deps: &[&THUNAR],
    },
    InstallableApp {
        name: "PCManFM",
        description: "LXDE file manager, simple and fast",
        deps: &[&PCMANFM],
    },
    InstallableApp {
        name: "PCManFM-Qt",
        description: "LXQt file manager with modern interface",
        deps: &[&PCMANFM_QT],
    },
    InstallableApp {
        name: "ranger",
        description: "Terminal file manager with vi-style navigation",
        deps: &[&RANGER],
    },
    InstallableApp {
        name: "lf",
        description: "Fast terminal file manager written in Go",
        deps: &[&LF],
    },
];

// =============================================================================
// Web Browsers (using legacy packages for now)
// =============================================================================

pub static FIREFOX: Dependency = Dependency {
    name: "Firefox",
    description: Some("Privacy-focused open source browser from Mozilla"),
    packages: &[
        PackageDefinition::new("firefox", PackageManager::Pacman),
        PackageDefinition::new("firefox", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("firefox")],
};

pub static CHROMIUM: Dependency = Dependency {
    name: "Chromium",
    description: Some("Open source foundation for Google Chrome"),
    packages: &[
        PackageDefinition::new("chromium", PackageManager::Pacman),
        PackageDefinition::new("chromium-browser", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("chromium")],
};

pub static FALKON: Dependency = Dependency {
    name: "Falkon",
    description: Some("Qt-based browser with KDE integration"),
    packages: &[
        PackageDefinition::new("falkon", PackageManager::Pacman),
        PackageDefinition::new("falkon", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("falkon")],
};

pub static EPIPHANY: Dependency = Dependency {
    name: "GNOME Web (Epiphany)",
    description: Some("Simple and clean browser for GNOME desktop"),
    packages: &[
        PackageDefinition::new("epiphany", PackageManager::Pacman),
        PackageDefinition::new("epiphany-browser", PackageManager::Apt),
    ],
    tests: &[InstallTest::WhichSucceeds("epiphany")],
};

pub static WEB_BROWSERS: &[InstallableApp] = &[
    InstallableApp {
        name: "Firefox",
        description: "Privacy-focused open source browser from Mozilla",
        deps: &[&FIREFOX],
    },
    InstallableApp {
        name: "Chromium",
        description: "Open source foundation for Google Chrome",
        deps: &[&CHROMIUM],
    },
    InstallableApp {
        name: "Falkon",
        description: "Qt-based browser with KDE integration",
        deps: &[&FALKON],
    },
    InstallableApp {
        name: "GNOME Web",
        description: "Simple and clean browser for GNOME desktop",
        deps: &[&EPIPHANY],
    },
];

// =============================================================================
// Archive Managers
// =============================================================================

dep!(FILE_ROLLER, "File Roller", "file-roller");
dep!(ARK, "Ark", "ark");
dep!(ENGRAMPA, "Engrampa", "engrampa");
dep!(XARCHIVER, "Xarchiver", "xarchiver");
dep!(LXQT_ARCHIVER, "LXQt Archiver", "lxqt-archiver");

pub static ARCHIVE_MANAGERS: &[InstallableApp] = &[
    InstallableApp {
        name: "File Roller",
        description: "GNOME archive manager",
        deps: &[&FILE_ROLLER],
    },
    InstallableApp {
        name: "Ark",
        description: "KDE archive manager",
        deps: &[&ARK],
    },
    InstallableApp {
        name: "Engrampa",
        description: "MATE archive manager",
        deps: &[&ENGRAMPA],
    },
    InstallableApp {
        name: "Xarchiver",
        description: "Lightweight GTK archive manager",
        deps: &[&XARCHIVER],
    },
    InstallableApp {
        name: "LXQt Archiver",
        description: "Qt-based lightweight archive manager",
        deps: &[&LXQT_ARCHIVER],
    },
];

// =============================================================================
// Install More Menu Helper
// =============================================================================

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
        for dep in self.app.deps {
            let status = if dep.is_installed() {
                NerdFont::Check
            } else {
                NerdFont::Circle
            };
            preview.push_str(&format!("  {} {}\n", status, dep.name));
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

            let deps: Vec<_> = item.app.deps.to_vec();
            match ensure_all(&deps)? {
                InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(true),
                InstallResult::Declined => Ok(false),
                InstallResult::NotAvailable { name, hint } => {
                    FzfWrapper::builder()
                        .message(format!("{} is not available:\n{}", name, hint))
                        .title("Package Not Available")
                        .show_message()?;
                    Ok(false)
                }
                InstallResult::Failed { reason } => {
                    FzfWrapper::builder()
                        .message(format!("Installation failed: {}", reason))
                        .title("Installation Failed")
                        .show_message()?;
                    Ok(false)
                }
            }
        }
        FzfResult::MultiSelected(_) => {
            // Multi-selection not used, treat as cancelled
            Ok(false)
        }
        FzfResult::Cancelled | FzfResult::Error(_) => Ok(false),
    }
}
