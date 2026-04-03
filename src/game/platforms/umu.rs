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

use super::LaunchCommandBuilderContext;
use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{WINDOWS_EXTENSIONS, validate_game_file};

pub struct UmuBuilder;

impl UmuBuilder {
    /// Build a Wine/umu-run launch command interactively
    pub fn build_command(
        context: Option<&LaunchCommandBuilderContext>,
    ) -> Result<Option<LaunchCommand>> {
        let prefix_hint = context
            .and_then(|ctx| {
                ctx.presets
                    .iter()
                    .find(|preset| preset.launcher == super::LauncherType::UmuRun)
            })
            .and_then(|preset| match &preset.data {
                super::BuilderPresetData::WinePrefix(path) => Some(path.as_path()),
                _ => None,
            });
        let executable_hint = context.and_then(|ctx| ctx.executable_path.as_deref());

        let runner = match Self::select_runner()? {
            Some(runner) => runner,
            None => return Ok(None),
        };

        // Step 1: Select Wine prefix
        let wine_prefix = match Self::select_wine_prefix(prefix_hint, executable_hint)? {
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
        let executable = match Self::select_executable(Some(&wine_prefix), executable_hint)? {
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

    fn select_wine_prefix(
        prefix_hint: Option<&Path>,
        executable_hint: Option<&Path>,
    ) -> Result<Option<PathBuf>> {
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

        let mut builder = PathInputBuilder::new()
            .header(header)
            .scope(FilePickerScope::Directories)
            .start_dir(
                Self::prefix_picker_start_dir(prefix_hint, executable_hint)
                    .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))),
            )
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
            ));

        if let Some(prefix_hint) = prefix_hint {
            builder = builder.start_path(prefix_hint.to_path_buf());
        }

        let selection = builder.choose()?;

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

    fn select_executable(
        prefix_hint: Option<&Path>,
        executable_hint: Option<&Path>,
    ) -> Result<Option<PathBuf>> {
        let mut prompt = FileSelectionPrompt::new(
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
        )
        .suggested_paths(Self::executable_suggestions(prefix_hint, executable_hint));

        if let Some(start_dir) = Self::executable_picker_start_dir(prefix_hint, executable_hint) {
            prompt = prompt.start_dir(start_dir);
        }

        if let Some(executable_hint) = executable_hint {
            prompt = prompt.start_path(executable_hint.to_path_buf());
        }

        select_file_with_validation(prompt, |path| {
            validate_game_file(path, "umu-run", WINDOWS_EXTENSIONS)
        })
    }

    fn prefix_picker_start_dir(
        prefix_hint: Option<&Path>,
        executable_hint: Option<&Path>,
    ) -> Option<PathBuf> {
        executable_hint
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| prefix_hint.and_then(|path| path.parent().map(Path::to_path_buf)))
            .or_else(|| prefix_hint.map(Path::to_path_buf))
    }

    fn executable_picker_start_dir(
        prefix_hint: Option<&Path>,
        executable_hint: Option<&Path>,
    ) -> Option<PathBuf> {
        executable_hint
            .and_then(|path| {
                if path.is_dir() {
                    Some(path.to_path_buf())
                } else {
                    path.parent().map(Path::to_path_buf)
                }
            })
            .or_else(|| {
                Self::executable_suggestions(prefix_hint, executable_hint)
                    .into_iter()
                    .find(|path| path.is_dir())
            })
    }

    fn executable_suggestions(
        prefix_hint: Option<&Path>,
        executable_hint: Option<&Path>,
    ) -> Vec<PathBuf> {
        let mut candidates = Vec::new();

        if let Some(executable_hint) = executable_hint {
            candidates.push(executable_hint.to_path_buf());
            if let Some(parent) = executable_hint.parent() {
                candidates.push(parent.to_path_buf());
            }
        }

        if let Some(prefix_hint) = prefix_hint {
            let drive_c = prefix_hint.join("drive_c");
            candidates.push(drive_c.join("Games"));
            candidates.push(drive_c.join("Program Files (x86)"));
            candidates.push(drive_c.join("Program Files"));
            candidates.push(drive_c.clone());

            if matches!(
                prefix_hint.file_name().and_then(|name| name.to_str()),
                Some("prefix" | "pfx")
            ) && let Some(parent) = prefix_hint.parent()
            {
                candidates.push(parent.to_path_buf());
            }
        }

        let mut deduped = Vec::new();
        for candidate in candidates {
            if deduped
                .iter()
                .any(|existing: &PathBuf| existing == &candidate)
            {
                continue;
            }
            if candidate.exists() {
                deduped.push(candidate);
            }
        }
        deduped
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_suggestions_prioritize_existing_game_adjacent_locations() {
        let temp = tempfile::tempdir().unwrap();
        let game_root = temp.path().join("MyGame");
        let prefix = game_root.join("prefix");
        let exe = game_root.join("Game.exe");

        std::fs::create_dir_all(prefix.join("drive_c").join("Games")).unwrap();
        std::fs::create_dir_all(prefix.join("drive_c").join("Program Files")).unwrap();
        std::fs::write(&exe, b"").unwrap();

        let suggestions = UmuBuilder::executable_suggestions(Some(&prefix), Some(&exe));

        assert_eq!(suggestions.first(), Some(&exe));
        assert!(suggestions.contains(&game_root));
        assert!(suggestions.contains(&prefix.join("drive_c").join("Games")));
    }

    #[test]
    fn prefix_picker_start_dir_prefers_executable_parent() {
        let temp = tempfile::tempdir().unwrap();
        let game_root = temp.path().join("MyGame");
        let prefix = game_root.join("prefix");
        let exe = game_root.join("Game.exe");

        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::write(&exe, b"").unwrap();

        let start_dir = UmuBuilder::prefix_picker_start_dir(Some(&prefix), Some(&exe));

        assert_eq!(start_dir, Some(game_root));
    }
}
