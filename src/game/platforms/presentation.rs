//! Emulator help shared by the launcher menu and installation chooser.

use crate::game::launch_command::{EmulatorLauncher, EmulatorPlatform};
use crate::menu_utils::FzfPreview;
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(super) fn style(platform: EmulatorPlatform) -> (NerdFont, &'static str) {
    match platform {
        EmulatorPlatform::Eden => (NerdFont::Gamepad, colors::GREEN),
        EmulatorPlatform::Dolphin => (NerdFont::Fish, colors::BLUE),
        EmulatorPlatform::Pcsx2 => (NerdFont::Disc, colors::SAPPHIRE),
        EmulatorPlatform::Azahar => (NerdFont::Gamepad, colors::YELLOW),
        EmulatorPlatform::Mgba => (NerdFont::Gamepad, colors::LAVENDER),
        EmulatorPlatform::DuckStation => (NerdFont::Disc, colors::PEACH),
    }
}

pub(super) fn launcher_preview(platform: EmulatorPlatform, reason: Option<&str>) -> FzfPreview {
    let (icon, _) = style(platform);
    let mut preview = PreviewBuilder::new()
        .header(icon, platform.name())
        .text(&format!("{} emulator.", platform.system_name()));
    if let Some(reason) = reason {
        preview = preview.blank().field("Recommended because", reason);
    }
    preview = preview
        .blank()
        .text("Supported formats:")
        .bullets(
            platform
                .extensions()
                .iter()
                .map(|extension| format!(".{extension} - {}", format_description(extension))),
        )
        .blank()
        .separator()
        .blank()
        .text("Installation:");
    if let Some(command) = platform.native_command() {
        preview = preview.field("Native command", command);
    }
    if let Some(app_id) = platform.flatpak_id() {
        preview = preview.bullet(&format!("flatpak install flathub {app_id}"));
    }
    preview = preview
        .text("AppImages are detected in:")
        .bullets(
            super::appimage_finder::SEARCH_DIRECTORIES
                .iter()
                .map(|directory| format!("~/{directory}")),
        )
        .text("You can also select an AppImage manually.");
    match platform {
        EmulatorPlatform::Eden => {
            preview = preview.field("Example AppImage", "~/AppImages/eden.AppImage");
        }
        EmulatorPlatform::Pcsx2 => {
            preview = preview.text("EmuDeck AppImages in ~/Emulation/tools are also detected.");
        }
        EmulatorPlatform::DuckStation => {
            preview = preview
                .text("If no installation is found, you can download the x86_64 AppImage automatically.")
                .field("Download location", super::duckstation::DEFAULT_DUCKSTATION_PATH);
        }
        _ => {}
    }
    preview
        .blank()
        .subtext("Choose the installation that contains your saves and emulator settings.")
        .build()
}

fn format_description(extension: &str) -> &'static str {
    match extension {
        "iso" => "Standard disc image",
        "bin" => "Binary disc image",
        "cue" => "CD track layout / cue sheet",
        "img" => "Disc image",
        "chd" => "Compressed Hunks of Data",
        "cso" | "ciso" => "Compressed ISO image",
        "gz" => "Gzip compressed image",
        "wbfs" => "Wii Backup File System",
        "gcm" => "GameCube disc image",
        "gcz" => "Dolphin compressed image",
        "wad" => "WiiWare / Virtual Console",
        "dol" | "elf" | "irx" | "axf" => "Executable / homebrew",
        "nsp" => "Nintendo Submission Package",
        "xci" => "NX Card Image",
        "nca" => "Nintendo Content Archive",
        "3ds" | "cci" => "Nintendo 3DS cartridge image",
        "3dsx" => "Nintendo 3DS homebrew",
        "cia" => "CTR Importable Archive",
        "app" | "cxi" => "Nintendo 3DS application image",
        "gba" => "Game Boy Advance ROM",
        "gb" => "Game Boy ROM",
        "gbc" => "Game Boy Color ROM",
        "sgb" => "Super Game Boy ROM",
        "zip" | "7z" => "Compressed ROM archive",
        "pbp" => "PSP eboot image",
        "ecm" => "Error Code Model compressed image",
        "mds" => "Disc image descriptor",
        "psf" | "minipsf" => "PlayStation sound file",
        "m3u" => "Multi-disc playlist",
        _ => "Game file",
    }
}

pub(super) fn installation_preview(
    platform: EmulatorPlatform,
    launcher: &EmulatorLauncher,
) -> FzfPreview {
    let (icon, _) = style(platform);
    let preview = PreviewBuilder::new().header(icon, platform.name());
    let preview = match launcher {
        EmulatorLauncher::Native { command } => preview
            .text("Native installation")
            .blank()
            .field("Command", command),
        EmulatorLauncher::Flatpak { app_id } => preview
            .text("Flatpak installation")
            .blank()
            .field("App ID", app_id)
            .field("Command", &format!("flatpak run {app_id}")),
        EmulatorLauncher::AppImage { path } => preview
            .text("AppImage installation")
            .blank()
            .field("Path", &path.display().to_string()),
    };
    preview
        .blank()
        .text("Choose the installation that contains your saves and emulator settings.")
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emulator_keeps_format_explanations_and_installation_help() {
        for platform in EmulatorPlatform::ALL {
            let FzfPreview::Text(preview) =
                launcher_preview(platform, Some("Existing game command"))
            else {
                panic!("Emulator help must have a preview");
            };
            for extension in platform.extensions() {
                assert!(
                    preview.contains(&format!(".{extension} - ")),
                    "{platform:?}: {extension}"
                );
                assert_ne!(
                    format_description(extension),
                    "Game file",
                    "Explain {extension}"
                );
            }
            assert!(preview.contains("Existing game command"));
            if let Some(app_id) = platform.flatpak_id() {
                assert!(preview.contains(&format!("flatpak install flathub {app_id}")));
            }
            assert!(preview.contains("~/AppImages"));
        }
        let FzfPreview::Text(preview) = launcher_preview(EmulatorPlatform::DuckStation, None)
        else {
            panic!("DuckStation help must have a preview");
        };
        assert!(preview.contains("download the x86_64 AppImage automatically"));
        assert!(preview.contains(super::super::duckstation::DEFAULT_DUCKSTATION_PATH));
    }
}
