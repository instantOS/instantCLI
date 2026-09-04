//! umu-run launch command builder
//!
//! Builds commands for running Windows games via umu-run (Proton/Wine wrapper)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use crate::common::package::{InstallResult, ensure_all};
use crate::game::launch_command::{
    LaunchCommand, LaunchCommandKind, ProtonSelection, WineLaunchCommand, WineRunner,
};
use crate::menu_utils::{
    ConfirmResult, FilePickerBuilder, FilePickerScope, FzfWrapper, MenuPresentation,
    PathInputBuilder, PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

use super::LaunchCommandBuilderContext;
use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{WINDOWS_EXTENSIONS, has_valid_extension, validate_game_file};

/// Upper bound for discovered executables so the menu stays navigable.
const MAX_SUGGESTED_EXECUTABLES: usize = 20;

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

        if matches!(runner, WineRunner::UmuRun) {
            match ensure_all(super::deps::dependencies_for_wine_runner(runner))? {
                InstallResult::Installed | InstallResult::AlreadyInstalled => {}
                InstallResult::Declined => return Ok(None),
                InstallResult::NotAvailable { hint, .. } => {
                    return Err(anyhow!("umu-launcher is not available: {}", hint));
                }
                InstallResult::Failed { reason } => {
                    return Err(anyhow!("umu-launcher installation failed: {}", reason));
                }
            }
        }

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
            .responsive_layout()
            .presentation(MenuPresentation::Padded)
            .select_one(options)?
        {
            crate::menu_utils::DialogOutcome::Submitted(item) if item.contains("umu-run") => {
                Ok(Some(WineRunner::UmuRun))
            }
            crate::menu_utils::DialogOutcome::Submitted(item) if item.contains("wine") => {
                Ok(Some(WineRunner::Wine))
            }
            crate::menu_utils::DialogOutcome::Submitted(_)
            | crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
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
            .responsive_layout()
            .presentation(MenuPresentation::Padded)
            .select_one(options.clone())?;

        match result {
            crate::menu_utils::DialogOutcome::Submitted(item) => {
                if item.contains("UMU-Proton") {
                    Ok(Some(ProtonSelection::UmuProtonLatest))
                } else if item.contains("GE-Proton") {
                    Ok(Some(ProtonSelection::GeProtonLatest))
                } else if item.contains("Custom") {
                    // Select custom proton path
                    match FilePickerBuilder::new()
                        .scope(FilePickerScope::Directories)
                        .pick_one()?
                    {
                        crate::menu_utils::DialogOutcome::Submitted(path) => {
                            Ok(Some(ProtonSelection::Custom(path)))
                        }
                        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
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
        // Only the instantly-known executable is static; discovered
        // executables stream in while the menu is already open.
        .suggested_paths(
            executable_hint
                .filter(|path| path.exists())
                .map(|path| vec![path.to_path_buf()])
                .unwrap_or_default(),
        );

        let scan_prefix = prefix_hint.map(Path::to_path_buf);
        let scan_hint = executable_hint.map(Path::to_path_buf);
        prompt = prompt.streaming_suggestions(Arc::new(move |sink| {
            UmuBuilder::for_each_discovered_executable(
                scan_prefix.as_deref(),
                scan_hint.as_deref(),
                &mut |path| sink.push(path),
            );
            for shortcut in UmuBuilder::directory_shortcuts(scan_prefix.as_deref()) {
                sink.push(shortcut);
            }
        }));

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
            .or_else(|| Self::directory_shortcuts(prefix_hint).into_iter().next())
    }

    /// All suggestions, including a synchronous discovery scan. The menu
    /// itself streams discoveries instead of waiting on this; this is the
    /// reference order: known executable, discovered executables, then
    /// directory shortcuts.
    #[cfg(test)]
    fn executable_suggestions(
        prefix_hint: Option<&Path>,
        executable_hint: Option<&Path>,
    ) -> Vec<PathBuf> {
        let mut suggestions: Vec<PathBuf> = executable_hint
            .filter(|path| path.exists())
            .map(|path| vec![path.to_path_buf()])
            .unwrap_or_default();

        Self::for_each_discovered_executable(prefix_hint, executable_hint, &mut |path| {
            if !suggestions.contains(&path) {
                suggestions.push(path);
            }
        });
        for shortcut in Self::directory_shortcuts(prefix_hint) {
            if !suggestions.contains(&shortcut) {
                suggestions.push(shortcut);
            }
        }
        suggestions
    }

    /// Likely game install locations inside a prefix, in priority order.
    /// Shared by executable discovery (scanned shallowly) and the directory
    /// browsing shortcuts so the two stay in sync.
    fn install_candidates(prefix_hint: &Path) -> Vec<PathBuf> {
        let drive_c = prefix_hint.join("drive_c");
        let mut candidates = vec![
            drive_c.join("Games"),
            drive_c.join("Program Files (x86)"),
            drive_c.join("Program Files"),
        ];
        if matches!(
            prefix_hint.file_name().and_then(|name| name.to_str()),
            Some("prefix" | "pfx")
        ) && let Some(parent) = prefix_hint.parent()
        {
            candidates.push(parent.to_path_buf());
        }
        candidates
    }

    /// Directory browsing shortcuts derived from the prefix. Paths are
    /// checked for existence, so this touches the filesystem — keep it off
    /// the menu's critical path.
    fn directory_shortcuts(prefix_hint: Option<&Path>) -> Vec<PathBuf> {
        let Some(prefix_hint) = prefix_hint else {
            return Vec::new();
        };

        let mut candidates = Self::install_candidates(prefix_hint);
        candidates.push(prefix_hint.join("drive_c"));

        let mut shortcuts = Vec::new();
        for candidate in candidates {
            if !shortcuts.contains(&candidate) && candidate.exists() {
                shortcuts.push(candidate);
            }
        }
        shortcuts
    }

    /// Feed every Windows executable found near the hint or inside the
    /// prefix's likely install locations to `emit`, most likely matches
    /// first. Scans are shallow and stop after `MAX_SUGGESTED_EXECUTABLES`
    /// finds.
    fn for_each_discovered_executable(
        prefix_hint: Option<&Path>,
        executable_hint: Option<&Path>,
        emit: &mut dyn FnMut(PathBuf),
    ) {
        let mut scan_roots: Vec<(PathBuf, usize)> = Vec::new();

        if let Some(parent) = executable_hint.and_then(Path::parent) {
            scan_roots.push((parent.to_path_buf(), 0));
        }

        if let Some(prefix_hint) = prefix_hint {
            for candidate in Self::install_candidates(prefix_hint) {
                scan_roots.push((candidate, 1));
            }
        }

        let mut budget = MAX_SUGGESTED_EXECUTABLES;
        for (root, depth) in scan_roots {
            if budget == 0 {
                break;
            }
            collect_windows_executables(&root, depth, &mut budget, emit);
        }
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

/// Collect Windows executable files under `dir`, descending at most
/// `max_depth` levels into subdirectories and emitting each find to `emit`.
/// Hidden directories are skipped, collection stops once the `budget` is
/// spent, and results are sorted for a stable menu order.
fn collect_windows_executables(
    dir: &Path,
    max_depth: usize,
    budget: &mut usize,
    emit: &mut dyn FnMut(PathBuf),
) {
    if *budget == 0 {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !is_hidden_path(&path) {
                subdirs.push(path);
            }
        } else if path.is_file() && has_valid_extension(&path, WINDOWS_EXTENSIONS) {
            files.push(path);
        }
    }

    files.sort();
    for file in files {
        if *budget == 0 {
            break;
        }
        emit(file);
        *budget -= 1;
    }

    if max_depth == 0 {
        return;
    }

    subdirs.sort();
    for subdir in subdirs {
        collect_windows_executables(&subdir, max_depth - 1, budget, emit);
        if *budget == 0 {
            break;
        }
    }
}

fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
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
    fn executable_suggestions_discover_executables_inside_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix");
        let game_exe = prefix.join("drive_c/Games/MyGame/Game.exe");
        let other_exe = prefix.join("drive_c/Program Files/Other/other.exe");
        std::fs::create_dir_all(game_exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(other_exe.parent().unwrap()).unwrap();
        std::fs::write(&game_exe, b"").unwrap();
        std::fs::write(&other_exe, b"").unwrap();

        let suggestions = UmuBuilder::executable_suggestions(Some(&prefix), None);

        assert_eq!(suggestions.first(), Some(&game_exe));
        assert!(suggestions.contains(&other_exe));

        // Discovered files come before the directory browsing fallbacks.
        let game_exe_index = suggestions
            .iter()
            .position(|path| path == &game_exe)
            .unwrap();
        let games_dir_index = suggestions
            .iter()
            .position(|path| path == &prefix.join("drive_c/Games"))
            .unwrap();
        assert!(game_exe_index < games_dir_index);
    }

    #[test]
    fn executable_discovery_skips_deep_and_hidden_locations() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix");
        let shallow = prefix.join("drive_c/Games/MyGame/Game.exe");
        let deep = prefix.join("drive_c/Games/MyGame/bin/Game.exe");
        let hidden = prefix.join("drive_c/Games/.hidden/Secret.exe");
        for path in [&shallow, &deep, &hidden] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"").unwrap();
        }

        let suggestions = UmuBuilder::executable_suggestions(Some(&prefix), None);

        assert!(suggestions.contains(&shallow));
        assert!(!suggestions.contains(&deep));
        assert!(!suggestions.contains(&hidden));
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
