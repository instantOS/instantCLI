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

use crate::game::launch_command::{GamescopeOptions, LaunchCommand};
use crate::game::utils::path::is_valid_wine_prefix;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{
    ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, TextEditOutcome, TextEditPrompt,
    prompt_text_edit,
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
    WinePrefix(PathBuf),
    SteamAppId(u32),
}

impl LaunchCommandBuilderContext {
    pub fn from_game(game_name: Option<&str>, save_path: Option<&Path>) -> Self {
        let mut presets = Vec::new();

        if let Some(path) = save_path {
            if let Some((app_id, prefix)) = infer_steam_prefix(path) {
                presets.push(LaunchCommandBuilderPreset {
                    launcher: LauncherType::Steam,
                    reason: format!("save path is inside Steam compatdata app ID {}", app_id),
                    data: BuilderPresetData::SteamAppId(app_id),
                });
                presets.push(LaunchCommandBuilderPreset {
                    launcher: LauncherType::UmuRun,
                    reason: "save path is inside a Proton prefix".to_string(),
                    data: BuilderPresetData::WinePrefix(prefix),
                });
            } else if let Some(prefix) = infer_wine_prefix(path) {
                presets.push(LaunchCommandBuilderPreset {
                    launcher: LauncherType::UmuRun,
                    reason: "save path is inside a Wine prefix".to_string(),
                    data: BuilderPresetData::WinePrefix(prefix),
                });
            }
        }

        Self {
            game_name: game_name.map(str::to_string),
            save_path: save_path.map(Path::to_path_buf),
            presets,
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
}

/// Launcher type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherType {
    Manual,
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
        LauncherType::UmuRun => UmuBuilder::build_command(context.and_then(|ctx| {
            ctx.preset_for(LauncherType::UmuRun)
                .and_then(|preset| match &preset.data {
                    BuilderPresetData::WinePrefix(path) => Some(path.as_path()),
                    _ => None,
                })
        })),
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

fn apply_launch_wrappers(mut command: LaunchCommand) -> Result<LaunchCommand> {
    command.wrappers.gamemode = ask_enable_gamemode()?;
    command.wrappers.gamescope = ask_gamescope_options()?;
    Ok(command)
}

fn ask_enable_gamemode() -> Result<bool> {
    match FzfWrapper::builder()
        .confirm(format!(
            "{} Wrap the launch with gamemoderun?",
            char::from(NerdFont::Performance)
        ))
        .yes_text("Use gamemode")
        .no_text("No gamemode")
        .confirm_dialog()?
    {
        ConfirmResult::Yes => Ok(true),
        ConfirmResult::No | ConfirmResult::Cancelled => Ok(false),
    }
}

fn ask_gamescope_options() -> Result<Option<GamescopeOptions>> {
    let enabled = match FzfWrapper::builder()
        .confirm(format!(
            "{} Wrap the launch with gamescope?",
            char::from(NerdFont::Desktop)
        ))
        .yes_text("Use gamescope")
        .no_text("No gamescope")
        .confirm_dialog()?
    {
        ConfirmResult::Yes => true,
        ConfirmResult::No | ConfirmResult::Cancelled => false,
    };

    if !enabled {
        return Ok(None);
    }

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

    Ok(Some(GamescopeOptions { options }))
}
