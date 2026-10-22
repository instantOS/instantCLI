//! Interactive launch command builder for games
//!
//! Emulator identities, supported inputs, and launcher commands are defined in
//! `launch_command::EmulatorPlatform`; all emulators use the same builder.

pub(crate) mod appimage_finder;
pub(crate) mod deps;
pub mod discovery;
mod duckstation;
mod emulator;
mod flatpak;
pub mod ludusavi;
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
    ChecklistResult, FilePickerBuilder, FzfSelectable, FzfWrapper, Header, TextEditOutcome,
    TextEditPrompt, prompt_text_edit,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub use steam_launcher::SteamBuilder;
pub use umu::UmuBuilder;

#[derive(Debug, Clone, Default)]
pub struct LaunchCommandBuilderContext {
    pub save_path: Option<PathBuf>,
    pub executable_path: Option<PathBuf>,
    pub presets: Vec<LaunchCommandBuilderPreset>,
    pub discovery_hint: Option<discovery::DiscoveryLaunchHint>,
    pub emulator_launcher: Option<(
        EmulatorPlatform,
        crate::game::launch_command::EmulatorLauncher,
    )>,
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
        save_path: Option<&Path>,
        existing_launch_command: Option<&LaunchCommand>,
    ) -> Self {
        let mut context = Self {
            save_path: save_path.map(Path::to_path_buf),
            executable_path: None,
            presets: Vec::new(),
            discovery_hint: None,
            emulator_launcher: None,
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
                    launcher: LauncherType::Emulator(EmulatorPlatform::Eden),
                    reason: "save path matches the Eden NAND save layout".to_string(),
                    data: BuilderPresetData::None,
                });
            }
        }

        context
    }

    pub fn add_discovery_hint(&mut self, hint: Option<&discovery::DiscoveryLaunchHint>) {
        let Some(hint) = hint else {
            return;
        };
        self.discovery_hint = Some(hint.clone());
        let (launcher, data) = match hint {
            discovery::DiscoveryLaunchHint::Wine { prefix } => (
                LauncherType::UmuRun,
                BuilderPresetData::WinePrefix(prefix.clone()),
            ),
            discovery::DiscoveryLaunchHint::Steam { app_id } => {
                (LauncherType::Steam, BuilderPresetData::SteamAppId(*app_id))
            }
            discovery::DiscoveryLaunchHint::Emulator { platform, game, .. } => {
                if self.executable_path.is_none() {
                    self.executable_path = game.clone();
                }
                (LauncherType::Emulator(*platform), BuilderPresetData::None)
            }
        };
        self.push_preset(LaunchCommandBuilderPreset {
            launcher,
            data,
            reason: "Discovered installation".to_string(),
        });
    }

    fn push_preset(&mut self, preset: LaunchCommandBuilderPreset) {
        if let Some(existing) = self
            .presets
            .iter_mut()
            .find(|existing| existing.launcher == preset.launcher)
        {
            if matches!(existing.data, BuilderPresetData::None)
                && !matches!(preset.data, BuilderPresetData::None)
            {
                *existing = preset;
            }
        } else {
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
                self.executable_path = Some(emulator.game.clone());
                self.emulator_launcher = Some((emulator.platform, emulator.launcher.clone()));
                self.discovery_hint = Some(discovery::DiscoveryLaunchHint::Emulator {
                    platform: emulator.platform,
                    source: if matches!(
                        emulator.launcher,
                        crate::game::launch_command::EmulatorLauncher::Flatpak { .. }
                    ) {
                        discovery::EmulatorSaveSource::Flatpak
                    } else {
                        discovery::EmulatorSaveSource::Native
                    },
                    game: Some(emulator.game.clone()),
                });
                let launcher = LauncherType::Emulator(emulator.platform);
                self.push_preset(LaunchCommandBuilderPreset {
                    launcher,
                    reason: format!(
                        "current launch command already uses {}",
                        launcher_recommendation_label(launcher)
                    ),
                    data: BuilderPresetData::None,
                });
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
    Emulator(EmulatorPlatform),
    Back,
}

impl std::fmt::Display for LauncherType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LauncherType::Manual => write!(f, "manual"),
            LauncherType::Executable => write!(f, "executable"),
            LauncherType::UmuRun => write!(f, "umu-run"),
            LauncherType::Steam => write!(f, "steam"),
            LauncherType::Emulator(platform) => write!(f, "{}", platform.id()),
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

    [
        LauncherType::Manual,
        LauncherType::Executable,
        LauncherType::UmuRun,
        LauncherType::Steam,
    ]
    .into_iter()
    .chain(
        EmulatorPlatform::ALL
            .into_iter()
            .map(LauncherType::Emulator),
    )
    .chain([LauncherType::Back])
    .map(|launcher| LauncherItem {
        launcher,
        display: launcher_display(launcher, recommended_launcher == Some(launcher)),
        preview: launcher_preview(launcher, context),
    })
    .collect()
}

fn launcher_display(launcher: LauncherType, recommended: bool) -> String {
    let suffix = if recommended { " [recommended]" } else { "" };
    match launcher {
        LauncherType::Manual => format!(
            "{} Manual Entry",
            format_icon_colored(NerdFont::Edit, colors::BLUE)
        ),
        LauncherType::Executable => format!(
            "{} Binary / Script",
            format_icon_colored(NerdFont::Code, colors::TEAL)
        ),
        LauncherType::UmuRun => format!(
            "{} Wine / umu-run{suffix}",
            format_icon_colored(NerdFont::Wine, colors::MAUVE)
        ),
        LauncherType::Steam => format!(
            "{} Steam{suffix}",
            format_icon_colored(NerdFont::Steam, colors::SAPPHIRE)
        ),
        LauncherType::Emulator(platform) => format!(
            "{} {} ({}){suffix}",
            format_icon_colored(NerdFont::Gamepad, colors::GREEN),
            platform.name(),
            platform.system_name(),
        ),
        LauncherType::Back => format!("{} Back", format_back_icon()),
    }
}

fn launcher_preview(
    launcher: LauncherType,
    context: Option<&LaunchCommandBuilderContext>,
) -> FzfPreview {
    match launcher {
        LauncherType::Manual => PreviewBuilder::new()
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
        LauncherType::Executable => PreviewBuilder::new()
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
        LauncherType::UmuRun => {
            let wine_preset = context.and_then(|c| c.preset_for(LauncherType::UmuRun));
            let recommended_wine_prefix = wine_preset.and_then(|preset| match &preset.data {
                BuilderPresetData::WinePrefix(path) => Some(path.display().to_string()),
                _ => None,
            });
            PreviewBuilder::new()
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
                .build()
        }
        LauncherType::Steam => {
            let steam_preset = context.and_then(|c| c.preset_for(LauncherType::Steam));
            let recommended_steam_app_id = steam_preset.and_then(|preset| match preset.data {
                BuilderPresetData::SteamAppId(app_id) => Some(app_id),
                _ => None,
            });
            PreviewBuilder::new()
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
                .build()
        }
        LauncherType::Emulator(platform) => {
            let preset = context.and_then(|ctx| ctx.preset_for(launcher));
            PreviewBuilder::new()
                .header(NerdFont::Gamepad, platform.name())
                .text(platform.system_name())
                .blank()
                .field(
                    "Supported files",
                    &validation::format_valid_extensions(platform.extensions()),
                )
                .text("Select an available native, Flatpak, or AppImage installation.")
                .field(
                    "Reason",
                    preset
                        .map(|preset| preset.reason.as_str())
                        .unwrap_or("<none>"),
                )
                .build()
        }
        LauncherType::Back => PreviewBuilder::new()
            .header(NerdFont::ArrowLeft, "Back")
            .text("Return to previous menu.")
            .build(),
    }
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
        .responsive_layout();

    if let Some(index) = initial_index {
        builder = builder.initial_index(index);
    }

    let result = builder.items(items).padded().select_one()?;

    match result {
        crate::menu_utils::DialogOutcome::Submitted(item) => {
            if item.launcher == LauncherType::Back {
                Ok(None)
            } else {
                Ok(Some(item.launcher))
            }
        }
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
    }
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
        LauncherType::Emulator(platform) => emulator::build_command(platform, context),
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

fn launcher_recommendation_label(launcher: LauncherType) -> &'static str {
    match launcher {
        LauncherType::Manual => "manual entry",
        LauncherType::Executable => "binary / script",
        LauncherType::UmuRun => "Wine / umu-run",
        LauncherType::Steam => "Steam",
        LauncherType::Emulator(platform) => platform.name(),
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
    fn inferred_wine_prefix_enriches_existing_preset() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("drive_c")).unwrap();
        let executable = temp.path().join("drive_c/game.exe");
        let command = LaunchCommand::from_shell_or_manual(format!("wine {}", executable.display()));
        let context = LaunchCommandBuilderContext::from_game(None, Some(&command));
        let preset = context.preset_for(LauncherType::UmuRun).unwrap();
        assert!(matches!(&preset.data, BuilderPresetData::WinePrefix(path) if path == temp.path()));
        assert_eq!(context.presets.len(), 1);
    }

    #[test]
    fn explicit_wine_prefix_takes_priority_over_inferred_save_prefix() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("drive_c")).unwrap();
        let command =
            LaunchCommand::from_shell_or_manual("WINEPREFIX=/explicit wine /games/game.exe");
        let context = LaunchCommandBuilderContext::from_game(
            Some(&temp.path().join("drive_c/save")),
            Some(&command),
        );
        assert!(
            matches!(&context.preset_for(LauncherType::UmuRun).unwrap().data, BuilderPresetData::WinePrefix(path) if path == Path::new("/explicit"))
        );
    }

    #[test]
    fn discovered_prefix_outside_save_directory_reaches_builder() {
        let hint = discovery::DiscoveryLaunchHint::Wine {
            prefix: "/prefixes/game".into(),
        };
        let mut context =
            LaunchCommandBuilderContext::from_game(Some(Path::new("/elsewhere/save")), None);
        context.add_discovery_hint(Some(&hint));
        assert_eq!(context.recommended_launcher(), Some(LauncherType::UmuRun));
        assert!(
            matches!(&context.preset_for(LauncherType::UmuRun).unwrap().data, BuilderPresetData::WinePrefix(path) if path == Path::new("/prefixes/game"))
        );
    }

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

        let context = LaunchCommandBuilderContext::from_game(None, Some(&command));

        assert_eq!(
            context.recommended_launcher(),
            Some(LauncherType::Emulator(EmulatorPlatform::Eden))
        );
    }

    #[test]
    fn recommends_eden_from_save_path_layout() {
        let save_path =
            Path::new("/home/test/.local/share/eden/nand/user/save/0000000000000000/abcdef");

        let context = LaunchCommandBuilderContext::from_game(Some(save_path), None);

        assert_eq!(
            context.recommended_launcher(),
            Some(LauncherType::Emulator(EmulatorPlatform::Eden))
        );
    }

    #[test]
    fn existing_launch_command_takes_priority_over_save_path_inference() {
        let command = LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Steam(SteamLaunchCommand { app_id: 12345 }),
        };
        let save_path =
            Path::new("/home/test/.local/share/eden/nand/user/save/0000000000000000/abcdef");

        let context = LaunchCommandBuilderContext::from_game(Some(save_path), Some(&command));

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
    let mut builder = FilePickerBuilder::new()
        .hint("Select a binary or script to launch the game")
        .show_hidden(false);

    if let Some(context) = context {
        if let Some(executable_path) = &context.executable_path {
            builder = builder.start_path(executable_path);
        } else if let Some(save_path) = &context.save_path
            && let Some(parent) = save_path.parent()
        {
            builder = builder.start_dir(parent);
        }
    }

    match builder.pick_one()? {
        crate::menu_utils::DialogOutcome::Submitted(path) => {
            Ok(Some(LaunchCommand::executable(&path)))
        }
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
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
        TextEditOutcome::Updated(Some(raw)) => shell_words::split(raw.trim()).unwrap_or_default(),
        _ => Vec::new(),
    };

    Ok(GamescopeOptions { options })
}
