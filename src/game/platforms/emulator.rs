use std::path::Path;

use anyhow::Result;

use crate::game::launch_command::{
    EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, EmulatorPlatform, LaunchCommand,
    LaunchCommandKind,
};
use crate::menu_utils::{ConfirmResult, DialogOutcome, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

use super::LaunchCommandBuilderContext;
use super::appimage_finder::find_appimages;
use super::flatpak::is_flatpak_app_installed;
use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_value, select_file_with_validation,
};
use super::validation::{format_valid_extensions, validate_game_file};

/// Deterministic preference: known AppImages, native command, then Flatpak.
/// Callers doing discovery can provide an installation hint to avoid switching
/// between independent emulator save/config directories.
pub(crate) fn detected_launchers(platform: EmulatorPlatform) -> Result<Vec<EmulatorLauncher>> {
    let mut launchers: Vec<_> = find_appimages(platform)
        .into_iter()
        .map(|path| EmulatorLauncher::AppImage { path })
        .collect();
    if let Some(command) = platform
        .native_command()
        .filter(|command| which::which(command).is_ok())
    {
        launchers.push(EmulatorLauncher::Native { command });
    }
    if let Some(app_id) = platform.flatpak_id()
        && is_flatpak_app_installed(app_id)?
    {
        launchers.push(EmulatorLauncher::Flatpak { app_id });
    }
    Ok(launchers)
}

#[derive(Clone)]
struct LauncherItem(EmulatorLauncher);

impl FzfSelectable for LauncherItem {
    fn fzf_display_text(&self) -> String {
        match &self.0 {
            EmulatorLauncher::Native { command } => format!("Native: {command}"),
            EmulatorLauncher::Flatpak { app_id } => format!("Flatpak: {app_id}"),
            EmulatorLauncher::AppImage { path } => format!("AppImage: {}", path.display()),
        }
    }
    fn fzf_key(&self) -> String {
        self.fzf_display_text()
    }
}

fn select_launcher(
    platform: EmulatorPlatform,
    context: Option<&LaunchCommandBuilderContext>,
) -> Result<Option<EmulatorLauncher>> {
    let mut launchers = detected_launchers(platform)?;
    if let Some((existing_platform, launcher)) =
        context.and_then(|ctx| ctx.emulator_launcher.as_ref())
        && *existing_platform == platform
        && let EmulatorLauncher::AppImage { path } = launcher
        && crate::common::TildePath::from_str(&path.to_string_lossy())
            .as_path()
            .is_file()
        && !launchers.contains(launcher)
    {
        launchers.insert(0, launcher.clone());
    }
    if let Some(super::discovery::DiscoveryLaunchHint::Emulator {
        platform: hinted_platform,
        source,
        ..
    }) = context.and_then(|ctx| ctx.discovery_hint.as_ref())
        && *hinted_platform == platform
    {
        launchers.retain(|launcher| source.matches(launcher));
        if launchers.is_empty() && *source == super::discovery::EmulatorSaveSource::Flatpak {
            FzfWrapper::message(&format!(
                "Install the {} Flatpak to use these discovered saves: flatpak install flathub {}",
                platform.name(),
                platform.flatpak_id().unwrap_or_default()
            ))?;
            return Ok(None);
        }
    }
    match launchers.as_slice() {
        [] => {
            if platform == EmulatorPlatform::DuckStation {
                return super::duckstation::select_or_download()
                    .map(|path| path.map(|path| EmulatorLauncher::AppImage { path }));
            }
            super::prompts::select_appimage_manually(platform.name())
                .map(|path| path.map(|path| EmulatorLauncher::AppImage { path }))
        }
        [launcher] => Ok(Some(launcher.clone())),
        _ => match FzfWrapper::builder()
            .prompt(platform.name())
            .header("Select emulator installation")
            .items(launchers.into_iter().map(LauncherItem).collect::<Vec<_>>())
            .select_one()?
        {
            DialogOutcome::Submitted(item) => Ok(Some(item.0)),
            DialogOutcome::Cancelled => Ok(None),
        },
    }
}

pub(super) fn build_command(
    platform: EmulatorPlatform,
    context: Option<&LaunchCommandBuilderContext>,
) -> Result<Option<LaunchCommand>> {
    let Some(launcher) = select_launcher(platform, context)? else {
        return Ok(None);
    };
    let mut prompt = FileSelectionPrompt::game_file(
        format!("Select {} game file", platform.system_name()),
        format!(
            "Select a game file ({})",
            format_valid_extensions(platform.extensions())
        ),
    );
    if let Some(path) = context.and_then(|ctx| ctx.executable_path.as_ref()) {
        prompt = prompt.start_path(path);
    }
    if platform == EmulatorPlatform::Eden {
        prompt = prompt.suggested_paths(super::discovery::eden::collect_configured_rom_files());
    }
    let Some(game) = select_file_with_validation(prompt, |path| {
        validate_game_file(path, platform.name(), platform.extensions())
    })?
    else {
        return Ok(None);
    };
    let batch_mode = if platform.supports_batch_mode() {
        match FzfWrapper::confirm(&format!(
            "{} Use {} batch mode?",
            char::from(NerdFont::Terminal),
            platform.name()
        ))? {
            ConfirmResult::Yes => true,
            ConfirmResult::No => false,
            ConfirmResult::Cancelled => return Ok(None),
        }
    } else {
        false
    };
    let Some(fullscreen) = ask_fullscreen()? else {
        return Ok(None);
    };
    confirm_value(launch_command(
        platform,
        launcher,
        &game,
        EmulatorOptions {
            batch_mode,
            fullscreen,
        },
    ))
}

pub(crate) fn launch_command(
    platform: EmulatorPlatform,
    launcher: EmulatorLauncher,
    game: &Path,
    options: EmulatorOptions,
) -> LaunchCommand {
    LaunchCommand {
        wrappers: Default::default(),
        kind: LaunchCommandKind::Emulator(EmulatorLaunchCommand {
            platform,
            launcher,
            game: game.to_path_buf(),
            options,
        }),
    }
}
