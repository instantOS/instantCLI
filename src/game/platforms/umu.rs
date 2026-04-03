//! umu-run launch command builder
//!
//! Builds commands for running Windows games via umu-run (Proton/Wine wrapper)

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::game::launch_command::{
    LaunchCommand, LaunchCommandKind, ProtonSelection, WineLaunchCommand, WineRunner,
};
use crate::menu_utils::{
    ConfirmResult, FilePickerResult, FilePickerScope, FzfWrapper, MenuWrapper, PathInputBuilder,
    PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{WINDOWS_EXTENSIONS, validate_game_file};

pub struct UmuBuilder;

impl UmuBuilder {
    /// Build a Wine/umu-run launch command interactively
    pub fn build_command(prefix_hint: Option<&Path>) -> Result<Option<LaunchCommand>> {
        let runner = match Self::select_runner()? {
            Some(runner) => runner,
            None => return Ok(None),
        };

        // Step 1: Select Wine prefix
        let wine_prefix = match Self::select_wine_prefix(prefix_hint)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let proton_path = if matches!(runner, WineRunner::UmuRun) {
            match Self::select_proton_version()? {
                Some(p) => p,
                None => return Ok(None),
            }
        } else {
            ProtonSelection::UmuProtonLatest
        };

        // Step 3: Select executable
        let executable = match Self::select_executable()? {
            Some(e) => e,
            None => return Ok(None),
        };

        // Step 4: Optional fullscreen flag
        let fullscreen = ask_fullscreen()?;

        // Build the command
        let command = Self::build_launch_command(
            runner,
            Some(wine_prefix),
            proton_path,
            &executable,
            fullscreen,
        );

        // Show preview and confirm
        let confirmed = confirm_command(&command)?;
        if confirmed {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn select_runner() -> Result<Option<WineRunner>> {
        let options = vec![
            format!("{} umu-run (recommended)", format_icon(NerdFont::Check)),
            format!("{} wine", format_icon(NerdFont::Wine)),
            format!("{} Cancel", format_icon(NerdFont::Cross)),
        ];

        match FzfWrapper::builder()
            .header(crate::menu_utils::Header::fancy("Select Wine Runner"))
            .prompt("Runner")
            .args(crate::ui::catppuccin::fzf_mocha_args())
            .responsive_layout()
            .select_padded(options)?
        {
            crate::menu_utils::FzfResult::Selected(item) if item.contains("umu-run") => {
                Ok(Some(WineRunner::UmuRun))
            }
            crate::menu_utils::FzfResult::Selected(item) if item.contains("wine") => {
                Ok(Some(WineRunner::Wine))
            }
            _ => Ok(None),
        }
    }

    fn select_wine_prefix(prefix_hint: Option<&Path>) -> Result<Option<PathBuf>> {
        if let Some(prefix_hint) = prefix_hint {
            match FzfWrapper::builder()
                .confirm(format!(
                    "{} Detected Wine/Proton prefix:\n{}\n\nUse this prefix?",
                    char::from(NerdFont::Check),
                    prefix_hint.display()
                ))
                .yes_text("Use detected prefix")
                .no_text("Choose different")
                .confirm_dialog()?
            {
                ConfirmResult::Yes => return Ok(Some(prefix_hint.to_path_buf())),
                ConfirmResult::Cancelled => return Ok(None),
                ConfirmResult::No => {}
            }
        }

        let header = prefix_hint
            .map(|path| {
                format!(
                    "{} Select Wine Prefix Directory\nDetected from save path: {}",
                    char::from(NerdFont::Wine),
                    path.display()
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "{} Select Wine Prefix Directory",
                    char::from(NerdFont::Wine)
                )
            });

        let selection = PathInputBuilder::new()
            .header(header)
            .scope(FilePickerScope::Directories)
            .picker_hint(format!(
                "{} Choose or create a Wine prefix directory",
                char::from(NerdFont::Info)
            ))
            .manual_option_label(format!(
                "{} Type prefix path manually",
                char::from(NerdFont::Edit)
            ))
            .picker_option_label(format!(
                "{} Browse for prefix directory",
                char::from(NerdFont::FolderOpen)
            ))
            .manual_prompt(
                prefix_hint
                    .map(|path| {
                        format!(
                            "{} Enter the prefix path [{}]:",
                            char::from(NerdFont::Edit),
                            path.display()
                        )
                    })
                    .unwrap_or_else(|| {
                        format!("{} Enter the prefix path:", char::from(NerdFont::Edit))
                    }),
            )
            .wine_prefix_option_label(format!(
                "{} Select from Wine prefixes",
                char::from(NerdFont::Wine)
            ))
            .choose()?;

        match selection {
            PathInputSelection::Manual(input) => {
                let path = PathBuf::from(shellexpand::tilde(&input).into_owned());
                if !path.exists() {
                    match FzfWrapper::confirm(&format!(
                        "{} Wine prefix '{}' does not exist. Create it?",
                        char::from(NerdFont::Warning),
                        path.display()
                    ))? {
                        ConfirmResult::Yes => {
                            std::fs::create_dir_all(&path)
                                .context("Failed to create Wine prefix directory")?;
                        }
                        _ => return Ok(None),
                    }
                }
                Ok(Some(path))
            }
            PathInputSelection::Picker(path) => Ok(Some(path)),
            PathInputSelection::WinePrefix(path) => Ok(Some(path)),
            PathInputSelection::Cancelled => Ok(None),
        }
    }

    fn select_proton_version() -> Result<Option<ProtonSelection>> {
        let options = vec![
            format!(
                "{} UMU-Proton (default, recommended)",
                format_icon(NerdFont::Check)
            ),
            format!("{} GE-Proton (latest)", format_icon(NerdFont::Download)),
            format!("{} Custom Proton path", format_icon(NerdFont::Folder)),
            format!("{} Cancel", format_icon(NerdFont::Cross)),
        ];

        let result = FzfWrapper::builder()
            .header(crate::menu_utils::Header::fancy("Select Proton Version"))
            .prompt("Proton")
            .args(crate::ui::catppuccin::fzf_mocha_args())
            .responsive_layout()
            .select_padded(options.clone())?;

        match result {
            crate::menu_utils::FzfResult::Selected(item) => {
                if item.contains("UMU-Proton") {
                    Ok(Some(ProtonSelection::UmuProtonLatest))
                } else if item.contains("GE-Proton") {
                    Ok(Some(ProtonSelection::GeProtonLatest))
                } else if item.contains("Custom") {
                    // Select custom proton path
                    let result = MenuWrapper::file_picker()
                        .scope(FilePickerScope::Directories)
                        .pick()?;
                    match result {
                        FilePickerResult::Selected(path) => Ok(Some(ProtonSelection::Custom(path))),
                        _ => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn select_executable() -> Result<Option<PathBuf>> {
        select_file_with_validation(
            FileSelectionPrompt::new(
                format!(
                    "{} Select Windows Executable",
                    char::from(NerdFont::Windows)
                ),
                format!(
                    "{} Select the .exe file to run ({})",
                    char::from(NerdFont::Info),
                    super::validation::format_valid_extensions(WINDOWS_EXTENSIONS)
                ),
                format!("{} Type executable path", char::from(NerdFont::Edit)),
                format!("{} Browse for executable", char::from(NerdFont::FolderOpen)),
            ),
            |path| validate_game_file(path, "umu-run", WINDOWS_EXTENSIONS),
        )
    }

    fn build_launch_command(
        runner: WineRunner,
        wine_prefix: Option<PathBuf>,
        proton_path: ProtonSelection,
        executable: &Path,
        _fullscreen: bool,
    ) -> LaunchCommand {
        LaunchCommand {
            wrappers: Default::default(),
            kind: LaunchCommandKind::Wine(WineLaunchCommand {
                runner,
                prefix: wine_prefix,
                proton: proton_path,
                executable: executable.to_path_buf(),
            }),
        }
    }
}

fn format_icon(icon: NerdFont) -> String {
    format!("{}", char::from(icon))
}
