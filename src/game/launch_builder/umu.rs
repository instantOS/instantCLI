//! umu-run launch command builder
//!
//! Builds commands for running Windows games via umu-run (Proton/Wine wrapper)

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::menu_utils::{
    ConfirmResult, FilePickerResult, FilePickerScope, FzfWrapper, MenuWrapper, PathInputBuilder,
    PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;

use super::prompts::{
    FileSelectionPrompt, ask_fullscreen, confirm_command, select_file_with_validation,
};
use super::validation::{WINDOWS_EXTENSIONS, validate_windows_executable};

/// Proton version selection
#[derive(Debug, Clone)]
pub enum ProtonPath {
    /// Use latest GE-Proton (auto-download)
    GeProtonLatest,
    /// Use latest UMU-Proton (default)
    UmuProtonLatest,
    /// Custom path to Proton installation
    Custom(PathBuf),
}

impl ProtonPath {
    fn to_env_value(&self) -> String {
        match self {
            ProtonPath::GeProtonLatest => "GE-Proton".to_string(),
            ProtonPath::UmuProtonLatest => String::new(), // Empty = default UMU-Proton
            ProtonPath::Custom(path) => path.to_string_lossy().to_string(),
        }
    }

    fn display_name(&self) -> &str {
        match self {
            ProtonPath::GeProtonLatest => "GE-Proton (latest)",
            ProtonPath::UmuProtonLatest => "UMU-Proton (default)",
            ProtonPath::Custom(_) => "Custom path",
        }
    }
}

pub struct UmuBuilder;

impl UmuBuilder {
    /// Build an umu-run launch command interactively
    pub fn build_command() -> Result<Option<String>> {
        // Step 1: Select Wine prefix
        let wine_prefix = match Self::select_wine_prefix()? {
            Some(p) => p,
            None => return Ok(None),
        };

        // Step 2: Select Proton version
        let proton_path = match Self::select_proton_version()? {
            Some(p) => p,
            None => return Ok(None),
        };

        // Step 3: Select executable
        let executable = match Self::select_executable()? {
            Some(e) => e,
            None => return Ok(None),
        };

        // Step 4: Optional fullscreen flag
        let fullscreen = ask_fullscreen()?;

        // Build the command
        let command = Self::format_command(&wine_prefix, &proton_path, &executable, fullscreen);

        // Show preview and confirm
        let confirmed = confirm_command(&command)?;
        if confirmed {
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn select_wine_prefix() -> Result<Option<PathBuf>> {
        let selection = PathInputBuilder::new()
            .header(format!(
                "{} Select Wine Prefix Directory",
                char::from(NerdFont::Wine)
            ))
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

    fn select_proton_version() -> Result<Option<ProtonPath>> {
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
                    Ok(Some(ProtonPath::UmuProtonLatest))
                } else if item.contains("GE-Proton") {
                    Ok(Some(ProtonPath::GeProtonLatest))
                } else if item.contains("Custom") {
                    // Select custom proton path
                    let result = MenuWrapper::file_picker()
                        .scope(FilePickerScope::Directories)
                        .pick()?;
                    match result {
                        FilePickerResult::Selected(path) => Ok(Some(ProtonPath::Custom(path))),
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
            validate_windows_executable,
        )
    }

    fn format_command(
        wine_prefix: &PathBuf,
        proton_path: &ProtonPath,
        executable: &PathBuf,
        _fullscreen: bool,
    ) -> String {
        let prefix_str = wine_prefix.to_string_lossy();
        let exe_str = executable.to_string_lossy();

        let proton_env = proton_path.to_env_value();

        let mut parts = Vec::new();

        // WINEPREFIX
        parts.push(format!("WINEPREFIX=\"{}\"", prefix_str));

        // PROTONPATH (only if not default)
        if !proton_env.is_empty() {
            parts.push(format!("PROTONPATH=\"{}\"", proton_env));
        }

        // The command
        parts.push(format!("umu-run \"{}\"", exe_str));

        parts.join(" ")
    }
}

fn format_icon(icon: NerdFont) -> String {
    format!("{}", char::from(icon))
}
