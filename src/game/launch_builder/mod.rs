//! Interactive launch command builder for games
//!
//! Supports building launch commands for:
//! - umu-run (Wine/Proton games)
//! - Eden (Switch emulator)
//! - Dolphin via Flatpak (GameCube/Wii emulator)
//! - PCSX2 via Flatpak (PlayStation 2 emulator)
//! - mGBA-Qt (Game Boy Advance emulator)
//! - DuckStation (PlayStation 1 emulator)

mod dolphin;
mod duckstation;
mod eden;
mod mgba;
mod pcsx2;
mod umu;
mod validation;

use anyhow::Result;

use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub use dolphin::DolphinBuilder;
pub use duckstation::DuckStationBuilder;
pub use eden::EdenBuilder;
pub use mgba::MgbaBuilder;
pub use pcsx2::Pcsx2Builder;
pub use umu::UmuBuilder;

/// Launcher type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherType {
    UmuRun,
    Eden,
    DolphinFlatpak,
    Pcsx2Flatpak,
    MgbaQt,
    DuckStation,
    Back,
}

impl std::fmt::Display for LauncherType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LauncherType::UmuRun => write!(f, "umu-run"),
            LauncherType::Eden => write!(f, "eden"),
            LauncherType::DolphinFlatpak => write!(f, "dolphin-flatpak"),
            LauncherType::Pcsx2Flatpak => write!(f, "pcsx2-flatpak"),
            LauncherType::MgbaQt => write!(f, "mgba-qt"),
            LauncherType::DuckStation => write!(f, "duckstation"),
            LauncherType::Back => write!(f, "back"),
        }
    }
}

#[derive(Clone)]
struct LauncherItem {
    launcher: LauncherType,
    display: String,
    preview: FzfPreview,
}

impl FzfSelectable for LauncherItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.launcher.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

/// Build launcher selection menu items
fn build_launcher_items() -> Vec<LauncherItem> {
    vec![
        LauncherItem {
            launcher: LauncherType::UmuRun,
            display: format!(
                "{} umu-run (Wine/Proton)",
                format_icon_colored(NerdFont::Wine, colors::MAUVE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Wine, "umu-run")
                .text("Unified launcher for Windows games on Linux.")
                .blank()
                .text("Run Windows executables (.exe) using Proton")
                .text("without requiring Steam.")
                .blank()
                .separator()
                .blank()
                .text("Configuration:")
                .bullet("Wine prefix directory")
                .bullet("Proton version (auto-downloads if needed)")
                .bullet("Windows executable path")
                .blank()
                .subtext("Requires: umu-launcher package")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::Eden,
            display: format!(
                "{} Eden (Switch Emulator)",
                format_icon_colored(NerdFont::Gamepad, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Gamepad, "Eden")
                .text("Nintendo Switch emulator.")
                .blank()
                .text("Runs Switch game files (.nsp, .xci, .nca)")
                .text("via the Eden AppImage.")
                .blank()
                .separator()
                .blank()
                .text("Default location:")
                .bullet("~/AppImages/eden.AppImage")
                .blank()
                .text("Supported formats:")
                .bullet(".nsp - Nintendo Submission Package")
                .bullet(".xci - NX Card Image")
                .bullet(".nca - Nintendo Content Archive")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::DolphinFlatpak,
            display: format!(
                "{} Dolphin Flatpak (GameCube/Wii)",
                format_icon_colored(NerdFont::Fish, colors::BLUE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Fish, "Dolphin (Flatpak)")
                .text("GameCube and Wii emulator.")
                .blank()
                .text("Runs GameCube/Wii games via the Flatpak")
                .text("version of Dolphin Emulator.")
                .blank()
                .separator()
                .blank()
                .text("Supported formats:")
                .bullet(".iso - Standard disc image")
                .bullet(".wbfs - Wii Backup File System")
                .bullet(".gcm - GameCube Master disc")
                .bullet(".ciso - Compressed ISO")
                .bullet(".gcz - Dolphin compressed format")
                .bullet(".wad - WiiWare/Virtual Console")
                .bullet(".dol/.elf - Homebrew executables")
                .blank()
                .subtext("Requires: org.DolphinEmu.dolphin-emu flatpak")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::Pcsx2Flatpak,
            display: format!(
                "{} PCSX2 Flatpak (PlayStation 2)",
                format_icon_colored(NerdFont::Disc, colors::SAPPHIRE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Disc, "PCSX2 (Flatpak)")
                .text("PlayStation 2 emulator.")
                .blank()
                .text("Runs PS2 games via the Flatpak")
                .text("version of PCSX2.")
                .blank()
                .separator()
                .blank()
                .text("Supported formats:")
                .bullet(".iso - Standard disc image")
                .bullet(".bin - Binary disc image")
                .bullet(".chd - Compressed Hunks of Data")
                .bullet(".cso - Compressed ISO")
                .bullet(".gz - Gzip compressed")
                .bullet(".elf/.irx - Executables")
                .blank()
                .subtext("Requires: net.pcsx2.PCSX2 flatpak")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::MgbaQt,
            display: format!(
                "{} mGBA-Qt (Game Boy Advance)",
                format_icon_colored(NerdFont::Gamepad, colors::LAVENDER)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Gamepad, "mGBA-Qt")
                .text("Game Boy Advance emulator.")
                .blank()
                .text("Runs GBA, GB, and GBC games")
                .text("via the mGBA-Qt application.")
                .blank()
                .separator()
                .blank()
                .text("Supported formats:")
                .bullet(".gba - Game Boy Advance ROM")
                .bullet(".gb - Game Boy ROM")
                .bullet(".gbc - Game Boy Color ROM")
                .bullet(".sgb - Super Game Boy ROM")
                .bullet(".zip/.7z - Compressed archives")
                .blank()
                .subtext("Requires: mgba-qt package")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::DuckStation,
            display: format!(
                "{} DuckStation (PlayStation 1)",
                format_icon_colored(NerdFont::Disc, colors::PEACH)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Disc, "DuckStation")
                .text("PlayStation 1 emulator.")
                .blank()
                .text("Runs PS1 games via the DuckStation AppImage.")
                .text("(Downloads automatically if not found)")
                .blank()
                .separator()
                .blank()
                .text("Default location:")
                .bullet("~/AppImages/DuckStation-x64.AppImage")
                .blank()
                .text("Supported formats:")
                .bullet(".bin/.cue - CD image + cue sheet")
                .bullet(".iso - Standard ISO image")
                .bullet(".chd - Compressed Hunks of Data")
                .bullet(".pbp - PSP eboot format")
                .bullet(".m3u - Multi-disc playlist")
                .blank()
                .subtext("x86_64 only - auto-downloads AppImage")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::Back,
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to previous menu.")
                .build(),
        },
    ]
}

/// Interactive launcher type selection
pub fn select_launcher_type() -> Result<Option<LauncherType>> {
    let items = build_launcher_items();

    let result = FzfWrapper::builder()
        .header(Header::fancy("Select Launcher Type"))
        .prompt("Launcher")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select_padded(items)?;

    match result {
        FzfResult::Selected(item) => {
            if item.launcher == LauncherType::Back {
                Ok(None)
            } else {
                Ok(Some(item.launcher))
            }
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

/// Main entry point for the launch command builder
/// Returns the built command string if successful
pub fn build_launch_command() -> Result<Option<String>> {
    let launcher_type = match select_launcher_type()? {
        Some(t) => t,
        None => return Ok(None),
    };

    match launcher_type {
        LauncherType::UmuRun => UmuBuilder::build_command(),
        LauncherType::Eden => EdenBuilder::build_command(),
        LauncherType::DolphinFlatpak => DolphinBuilder::build_command(),
        LauncherType::Pcsx2Flatpak => Pcsx2Builder::build_command(),
        LauncherType::MgbaQt => MgbaBuilder::build_command(),
        LauncherType::DuckStation => DuckStationBuilder::build_command(),
        LauncherType::Back => Ok(None),
    }
}
