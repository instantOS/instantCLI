//! Interactive launch command builder for games
//!
//! Supports building launch commands for:
//! - umu-run (Wine/Proton games)
//! - Eden (Switch emulator)
//! - Dolphin via Flatpak (GameCube/Wii emulator)
//! - PCSX2 via Flatpak (PlayStation 2 emulator)
//! - mGBA-Qt (Game Boy Advance emulator)
//! - DuckStation (PlayStation 1 emulator)

pub(crate) mod appimage_finder;
mod azahar;
pub(crate) mod deps;
pub mod discovery;
mod dolphin;
mod duckstation;
mod eden;
mod flatpak;
pub mod ludusavi;
mod mgba;
mod pcsx2;
mod prompts;
mod steam_launcher;
mod umu;
mod validation;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::game::launch_command::{
    EmulatorPlatform, GamescopeOptions, LaunchCommand, LaunchCommandKind,
};
use crate::game::utils::path::is_valid_wine_prefix;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{
    ChecklistResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuWrapper, TextEditOutcome,
    TextEditPrompt, prompt_text_edit,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub use azahar::AzaharBuilder;
pub use dolphin::DolphinBuilder;
pub use duckstation::DuckStationBuilder;
pub use eden::EdenBuilder;
pub use mgba::MgbaBuilder;
pub use pcsx2::Pcsx2Builder;
pub use steam_launcher::SteamBuilder;
pub use umu::UmuBuilder;

#[derive(Debug, Clone, Default)]
pub struct LaunchCommandBuilderContext {
    pub game_name: Option<String>,
    pub save_path: Option<PathBuf>,
    pub executable_path: Option<PathBuf>,
    pub presets: Vec<LaunchCommandBuilderPreset>,
}

#[derive(Debug, Clone)]
pub struct LaunchCommandBuilderPreset {
    pub launcher: LauncherType,
    pub reason: String,
    pub data: BuilderPresetData,
}

#[derive(Debug, Clone)]
pub enum BuilderPresetData {
    None,
    WinePrefix(PathBuf),
    SteamAppId(u32),
}

impl LaunchCommandBuilderContext {
    pub fn from_game(
        game_name: Option<&str>,
        save_path: Option<&Path>,
        existing_launch_command: Option<&LaunchCommand>,
    ) -> Self {
        let mut context = Self {
            game_name: game_name.map(str::to_string),
            save_path: save_path.map(Path::to_path_buf),
            executable_path: None,
            presets: Vec::new(),
        };

        if let Some(command) = existing_launch_command {
            context.add_presets_from_launch_command(command);
        }

        if let Some(path) = save_path {
            if let Some((app_id, prefix)) = infer_steam_prefix(path) {
                context.push_preset(LaunchCommandBuilderPreset {
                    launcher: LauncherType::Steam,
                    reason: format!("save path is inside Steam compatdata app ID {}", app_id),
                    data: BuilderPresetData::SteamAppId(app_id),
                });
                context.push_preset(LaunchCommandBuilderPreset {
                    launcher: LauncherType::UmuRun,
                    reason: "save path is inside a Proton prefix".to_string(),
                    data: BuilderPresetData::WinePrefix(prefix),
                });
            } else if let Some(prefix) = infer_wine_prefix(path) {
                context.push_preset(LaunchCommandBuilderPreset {
                    launcher: LauncherType::UmuRun,
                    reason: "save path is inside a Wine prefix".to_string(),
                    data: BuilderPresetData::WinePrefix(prefix),
                });
            } else if is_eden_save_path(path) {
                context.push_preset(LaunchCommandBuilderPreset {
                    launcher: LauncherType::Eden,
                    reason: "save path matches the Eden NAND save layout".to_string(),
                    data: BuilderPresetData::None,
                });
            }
        }

        context
    }

    fn push_preset(&mut self, preset: LaunchCommandBuilderPreset) {
        if self
            .presets
            .iter()
            .all(|existing| existing.launcher != preset.launcher)
        {
            self.presets.push(preset);
        }
    }

    fn recommended_launcher(&self) -> Option<LauncherType> {
        self.presets.first().map(|preset| preset.launcher)
    }

    fn preset_for(&self, launcher: LauncherType) -> Option<&LaunchCommandBuilderPreset> {
        self.presets
            .iter()
            .find(|preset| preset.launcher == launcher)
    }

    fn add_presets_from_launch_command(&mut self, command: &LaunchCommand) {
        match &command.kind {
            LaunchCommandKind::Steam(steam) => {
                self.push_preset(LaunchCommandBuilderPreset {
                    launcher: LauncherType::Steam,
                    reason: format!(
                        "current launch command already uses Steam app ID {}",
                        steam.app_id
                    ),
                    data: BuilderPresetData::SteamAppId(steam.app_id),
                });
            }
            LaunchCommandKind::Wine(wine) => {
                self.executable_path = Some(wine.executable.clone());

                self.push_preset(LaunchCommandBuilderPreset {
                    launcher: LauncherType::UmuRun,
                    reason: "current launch command already uses Wine/umu-run".to_string(),
                    data: wine
                        .prefix
                        .clone()
                        .map(BuilderPresetData::WinePrefix)
                        .unwrap_or(BuilderPresetData::None),
                });

                if wine.prefix.is_none()
                    && let Some(prefix) = infer_wine_prefix(&wine.executable)
                {
                    self.push_preset(LaunchCommandBuilderPreset {
                        launcher: LauncherType::UmuRun,
                        reason: "current Wine executable is inside a Wine prefix".to_string(),
                        data: BuilderPresetData::WinePrefix(prefix),
                    });
                }
            }
            LaunchCommandKind::Emulator(emulator) => {
                if let Some(launcher) = launcher_type_for_emulator(emulator.platform) {
                    self.push_preset(LaunchCommandBuilderPreset {
                        launcher,
                        reason: format!(
                            "current launch command already uses {}",
                            launcher_recommendation_label(launcher)
                        ),
                        data: BuilderPresetData::None,
                    });
                }
            }
            LaunchCommandKind::Manual { .. } => {}
        }
    }
}

/// Launcher type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherType {
    Manual,
    Executable,
    UmuRun,
    Steam,
    Eden,
    DolphinFlatpak,
    Pcsx2Flatpak,
    AzaharFlatpak,
    MgbaQt,
    DuckStation,
    Back,
}

impl std::fmt::Display for LauncherType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LauncherType::Manual => write!(f, "manual"),
            LauncherType::Executable => write!(f, "executable"),
            LauncherType::UmuRun => write!(f, "umu-run"),
            LauncherType::Steam => write!(f, "steam"),
            LauncherType::Eden => write!(f, "eden"),
            LauncherType::DolphinFlatpak => write!(f, "dolphin-flatpak"),
            LauncherType::Pcsx2Flatpak => write!(f, "pcsx2-flatpak"),
            LauncherType::AzaharFlatpak => write!(f, "azahar-flatpak"),
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
fn build_launcher_items(context: Option<&LaunchCommandBuilderContext>) -> Vec<LauncherItem> {
    let recommended_launcher = context.and_then(LaunchCommandBuilderContext::recommended_launcher);
    let wine_preset = context.and_then(|context| context.preset_for(LauncherType::UmuRun));
    let steam_preset = context.and_then(|context| context.preset_for(LauncherType::Steam));
    let eden_preset = context.and_then(|context| context.preset_for(LauncherType::Eden));

    let recommended_wine_prefix = wine_preset.and_then(|preset| match &preset.data {
        BuilderPresetData::WinePrefix(path) => Some(path.display().to_string()),
        _ => None,
    });
    let recommended_steam_app_id = steam_preset.and_then(|preset| match preset.data {
        BuilderPresetData::SteamAppId(app_id) => Some(app_id),
        _ => None,
    });

    vec![
        LauncherItem {
            launcher: LauncherType::Manual,
            display: format!(
                "{} Manual Entry",
                format_icon_colored(NerdFont::Edit, colors::BLUE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Edit, "Manual Entry")
                .text("Type the launch command directly.")
                .blank()
                .text("Enter any custom command to launch the game.")
                .text("Useful for scripts, custom emulators, or")
                .text("commands not covered by the builders.")
                .blank()
                .separator()
                .blank()
                .text("Examples:")
                .bullet("flatpak run com.valvesoftware.Steam")
                .bullet("./my-game-launcher.sh")
                .bullet("/usr/games/my-emulator game.rom")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::Executable,
            display: format!(
                "{} Binary / Script",
                format_icon_colored(NerdFont::Code, colors::TEAL)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Code, "Binary / Script")
                .text("Select a script or binary from your system.")
                .blank()
                .text("Open a file picker to choose a game")
                .text("executable or launcher script.")
                .blank()
                .separator()
                .blank()
                .text("Use this for:")
                .bullet("Native Linux binaries")
                .bullet("Shell scripts (.sh)")
                .bullet("Python scripts (.py)")
                .bullet("AppImages (manual selection)")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::UmuRun,
            display: format!(
                "{} Wine / umu-run{}",
                format_icon_colored(NerdFont::Wine, colors::MAUVE),
                if recommended_launcher == Some(LauncherType::UmuRun) {
                    " [recommended]"
                } else {
                    ""
                }
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Wine, "Wine / umu-run")
                .text("Typed Windows launcher variant.")
                .blank()
                .text("Run Windows executables (.exe) using umu-run")
                .text("or plain wine with a known prefix and executable.")
                .blank()
                .separator()
                .blank()
                .text("Configuration:")
                .bullet("Wine prefix directory")
                .bullet("Runner: umu-run or wine")
                .bullet("Proton version for umu-run")
                .bullet("Windows executable path")
                .blank()
                .field(
                    "Recommended prefix",
                    recommended_wine_prefix
                        .as_deref()
                        .unwrap_or("<none inferred>"),
                )
                .field(
                    "Reason",
                    wine_preset
                        .map(|preset| preset.reason.as_str())
                        .unwrap_or("<none>"),
                )
                .blank()
                .subtext("Requires: umu-launcher for the umu-run mode")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::Steam,
            display: format!(
                "{} Steam{}",
                format_icon_colored(NerdFont::Steam, colors::SAPPHIRE),
                if recommended_launcher == Some(LauncherType::Steam) {
                    " [recommended]"
                } else {
                    ""
                }
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Steam, "Steam")
                .text("Native Steam launch command variant.")
                .blank()
                .text("Uses a typed Steam app ID and serializes to")
                .text("a standard `steam://rungameid/...` command.")
                .blank()
                .field(
                    "Detected app ID",
                    &recommended_steam_app_id
                        .map(|app_id| app_id.to_string())
                        .unwrap_or_else(|| "<none inferred>".to_string()),
                )
                .field(
                    "Reason",
                    steam_preset
                        .map(|preset| preset.reason.as_str())
                        .unwrap_or("<none>"),
                )
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::Eden,
            display: format!(
                "{} Eden (Switch Emulator){}",
                format_icon_colored(NerdFont::Gamepad, colors::GREEN),
                if recommended_launcher == Some(LauncherType::Eden) {
                    " [recommended]"
                } else {
                    ""
                }
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
                .blank()
                .field(
                    "Reason",
                    eden_preset
                        .map(|preset| preset.reason.as_str())
                        .unwrap_or("<none>"),
                )
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
                "{} PCSX2 (PlayStation 2)",
                format_icon_colored(NerdFont::Disc, colors::SAPPHIRE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Disc, "PCSX2")
                .text("PlayStation 2 emulator.")
                .blank()
                .text("Runs PS2 games via EmuDeck AppImage")
                .text("(auto-detected) or Flatpak fallback.")
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
                .separator()
                .blank()
                .text("Installation:")
                .bullet("Preferred: EmuDeck (includes AppImage)")
                .bullet("Fallback: flatpak install flathub net.pcsx2.PCSX2")
                .build(),
        },
        LauncherItem {
            launcher: LauncherType::AzaharFlatpak,
            display: format!(
                "{} Azahar Flatpak (Nintendo 3DS)",
                format_icon_colored(NerdFont::Gamepad, colors::YELLOW)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Gamepad, "Azahar (Flatpak)")
                .text("Nintendo 3DS emulator.")
                .blank()
                .text("Runs 3DS games via the Flatpak")
                .text("version of Azahar (Citra fork).")
                .blank()
                .separator()
                .blank()
                .text("Supported formats:")
                .bullet(".3ds - Standard 3DS ROM")
                .bullet(".3dsx - Homebrew format")
                .bullet(".cia - CTR Importable Archive")
                .bullet(".app/.elf - Executables")
                .bullet(".cci/.cxi - Cartridge images")
                .blank()
                .subtext("Requires: org.azahar_emu.Azahar flatpak")
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
pub fn select_launcher_type(
    context: Option<&LaunchCommandBuilderContext>,
) -> Result<Option<LauncherType>> {
    let items = build_launcher_items(context);
    let recommended = context.and_then(LaunchCommandBuilderContext::recommended_launcher);
    let initial_index =
        recommended.and_then(|launcher| items.iter().position(|item| item.launcher == launcher));

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Select Launcher Type"))
        .prompt("Launcher")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = initial_index {
        builder = builder.initial_index(index);
    }

    let result = builder.select_padded(items)?;

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
/// Returns the typed launch command if successful
pub fn build_launch_command() -> Result<Option<LaunchCommand>> {
    build_launch_command_with_context(None)
}

pub fn build_launch_command_with_context(
    context: Option<&LaunchCommandBuilderContext>,
) -> Result<Option<LaunchCommand>> {
    let launcher_type = match select_launcher_type(context)? {
        Some(t) => t,
        None => return Ok(None),
    };

    let command = match launcher_type {
        LauncherType::Manual => prompt_manual_command(),
        LauncherType::Executable => prompt_executable_command(context),
        LauncherType::UmuRun => UmuBuilder::build_command(context),
        LauncherType::Steam => SteamBuilder::build_command(context.and_then(|ctx| {
            ctx.preset_for(LauncherType::Steam)
                .and_then(|preset| match preset.data {
                    BuilderPresetData::SteamAppId(app_id) => Some(app_id),
                    _ => None,
                })
        })),
        LauncherType::Eden => EdenBuilder::build_command(),
        LauncherType::DolphinFlatpak => DolphinBuilder::build_command(),
        LauncherType::Pcsx2Flatpak => Pcsx2Builder::build_command(),
        LauncherType::AzaharFlatpak => AzaharBuilder::build_command(),
        LauncherType::MgbaQt => MgbaBuilder::build_command(),
        LauncherType::DuckStation => DuckStationBuilder::build_command(),
        LauncherType::Back => Ok(None),
    }?;

    command.map(apply_launch_wrappers).transpose()
}

fn infer_wine_prefix(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| is_valid_wine_prefix(ancestor))
        .map(Path::to_path_buf)
}

fn infer_steam_prefix(path: &Path) -> Option<(u32, PathBuf)> {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|part| part.to_str()) != Some("pfx") {
            continue;
        }

        let app_id = ancestor
            .parent()?
            .file_name()?
            .to_str()?
            .parse::<u32>()
            .ok()?;

        if ancestor
            .parent()?
            .parent()?
            .file_name()
            .and_then(|part| part.to_str())
            == Some("compatdata")
        {
            return Some((app_id, ancestor.to_path_buf()));
        }
    }

    None
}

fn is_eden_save_path(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor
            .strip_prefix(Path::new("/"))
            .ok()
            .map(is_eden_save_suffix)
            .unwrap_or_else(|| is_eden_save_suffix(ancestor))
    })
}

fn is_eden_save_suffix(path: &Path) -> bool {
    let components: Vec<_> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    components.len() >= 7
        && components[components.len() - 7..]
            == [
                ".local".to_string(),
                "share".to_string(),
                "eden".to_string(),
                "nand".to_string(),
                "user".to_string(),
                "save".to_string(),
                "0000000000000000".to_string(),
            ]
}

fn launcher_type_for_emulator(platform: EmulatorPlatform) -> Option<LauncherType> {
    match platform {
        EmulatorPlatform::Dolphin => Some(LauncherType::DolphinFlatpak),
        EmulatorPlatform::Eden => Some(LauncherType::Eden),
        EmulatorPlatform::Azahar => Some(LauncherType::AzaharFlatpak),
        EmulatorPlatform::Mgba => Some(LauncherType::MgbaQt),
        EmulatorPlatform::Pcsx2 => Some(LauncherType::Pcsx2Flatpak),
        EmulatorPlatform::DuckStation => Some(LauncherType::DuckStation),
    }
}

fn launcher_recommendation_label(launcher: LauncherType) -> &'static str {
    match launcher {
        LauncherType::Manual => "manual entry",
        LauncherType::Executable => "binary / script",
        LauncherType::UmuRun => "Wine / umu-run",
        LauncherType::Steam => "Steam",
        LauncherType::Eden => "Eden",
        LauncherType::DolphinFlatpak => "Dolphin",
        LauncherType::Pcsx2Flatpak => "PCSX2",
        LauncherType::AzaharFlatpak => "Azahar",
        LauncherType::MgbaQt => "mGBA-Qt",
        LauncherType::DuckStation => "DuckStation",
        LauncherType::Back => "back",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::launch_command::{
        EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, SteamLaunchCommand,
    };

    #[test]
    fn recommends_eden_from_existing_launch_command() {
        let command = LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Eden,
                launcher: EmulatorLauncher::AppImage {
                    path: PathBuf::from("~/AppImages/eden.AppImage"),
                },
                game: PathBuf::from("/games/Test.xci"),
                options: EmulatorOptions::default(),
            }),
        };

        let context = LaunchCommandBuilderContext::from_game(None, None, Some(&command));

        assert_eq!(context.recommended_launcher(), Some(LauncherType::Eden));
    }

    #[test]
    fn recommends_eden_from_save_path_layout() {
        let save_path =
            Path::new("/home/test/.local/share/eden/nand/user/save/0000000000000000/abcdef");

        let context = LaunchCommandBuilderContext::from_game(None, Some(save_path), None);

        assert_eq!(context.recommended_launcher(), Some(LauncherType::Eden));
    }

    #[test]
    fn existing_launch_command_takes_priority_over_save_path_inference() {
        let command = LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Steam(SteamLaunchCommand { app_id: 12345 }),
        };
        let save_path =
            Path::new("/home/test/.local/share/eden/nand/user/save/0000000000000000/abcdef");

        let context = LaunchCommandBuilderContext::from_game(None, Some(save_path), Some(&command));

        assert_eq!(context.recommended_launcher(), Some(LauncherType::Steam));
    }
}

/// Prompt user to manually enter a launch command
fn prompt_manual_command() -> Result<Option<LaunchCommand>> {
    let prompt = TextEditPrompt::new("Launch command", None)
        .header("Enter the launch command")
        .ghost("e.g., flatpak run com.valvesoftware.Steam -applaunch 12345");

    match prompt_text_edit(prompt)? {
        TextEditOutcome::Updated(Some(command)) => {
            if command.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(LaunchCommand::from_shell_or_manual(
                    command.trim().to_string(),
                )))
            }
        }
        _ => Ok(None),
    }
}

/// Prompt user to select an executable file
fn prompt_executable_command(
    context: Option<&LaunchCommandBuilderContext>,
) -> Result<Option<LaunchCommand>> {
    let mut builder = MenuWrapper::file_picker()
        .hint("Select a binary or script to launch the game")
        .show_hidden(false);

    if let Some(context) = context {
        if let Some(executable_path) = &context.executable_path {
            builder = builder.start_path(executable_path);
        } else if let Some(save_path) = &context.save_path {
            if let Some(parent) = save_path.parent() {
                builder = builder.start_dir(parent);
            }
        }
    }

    match builder.pick_one()? {
        Some(path) => {
            let command = format!("'{}'", path.display());
            Ok(Some(LaunchCommand::from_shell_or_manual(command)))
        }
        None => Ok(None),
    }
}

fn apply_launch_wrappers(mut command: LaunchCommand) -> Result<LaunchCommand> {
    let selected = ask_launch_wrappers()?;

    command.wrappers.gamemode = selected.contains(&LaunchWrapperOption::Gamemode);

    if selected.contains(&LaunchWrapperOption::Gamescope) {
        command.wrappers.gamescope = Some(ask_gamescope_flags()?);
    }

    Ok(command)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchWrapperOption {
    Gamemode,
    Gamescope,
}

impl FzfSelectable for LaunchWrapperOption {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Gamemode => format!(
                "{} Gamemode (gamemoderun)",
                char::from(NerdFont::Performance)
            ),
            Self::Gamescope => format!(
                "{} Gamescope (micro-compositor)",
                char::from(NerdFont::Desktop)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Gamemode => PreviewBuilder::new()
                .header(NerdFont::Performance, "Gamemode")
                .line(colors::GREEN, Some(NerdFont::Performance), "Enable GameMode")
                .text("GameMode is a daemon/library combo for Linux that allows games to request a set of optimizations be temporarily applied to the host OS and/or a game process.")
                .build(),
            Self::Gamescope => PreviewBuilder::new()
                .header(NerdFont::Desktop, "Gamescope")
                .line(colors::PEACH, Some(NerdFont::Desktop), "Enable Gamescope")
                .text("Gamescope is a micro-compositor that provides features like upscaling, resolution control, and better performance for games.")
                .build(),
        }
    }
}

fn ask_launch_wrappers() -> Result<Vec<LaunchWrapperOption>> {
    let items = vec![
        LaunchWrapperOption::Gamemode,
        LaunchWrapperOption::Gamescope,
    ];

    match FzfWrapper::builder()
        .prompt("Launch Options")
        .header("Enter on item toggles it | Enter on Continue confirms")
        .checklist("Continue")
        .checklist_dialog(items)?
    {
        ChecklistResult::Confirmed(selected) => Ok(selected),
        _ => Ok(Vec::new()),
    }
}

fn ask_gamescope_flags() -> Result<GamescopeOptions> {
    let prompt = TextEditPrompt::new("Gamescope options", Some(""))
        .header("Enter optional gamescope flags")
        .ghost("Example: -f -W 1280 -H 720");

    let options = match prompt_text_edit(prompt)? {
        TextEditOutcome::Updated(Some(raw)) => match shell_words::split(raw.trim()) {
            Ok(options) => options,
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };

    Ok(GamescopeOptions { options })
}
