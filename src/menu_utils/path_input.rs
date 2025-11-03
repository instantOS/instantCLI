use std::path::PathBuf;

use anyhow::{Result, anyhow};

use super::file_picker::{FilePickerScope, MenuWrapper};
use super::fzf::{FzfResult, FzfSelectable, FzfWrapper};
use crate::dot::path_serde::TildePath;
use crate::ui::nerd_font::NerdFont;

#[derive(Debug, Clone)]
enum PathInputChoice {
    Manual,
    Picker,
    WinePrefix,
}

#[derive(Debug, Clone)]
struct PathInputOption {
    label: String,
    choice: PathInputChoice,
}

impl PathInputOption {
    fn new(label: String, choice: PathInputChoice) -> Self {
        Self { label, choice }
    }
}

impl FzfSelectable for PathInputOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_key(&self) -> String {
        self.label.clone()
    }
}

#[derive(Debug, Clone)]
pub struct PathInputBuilder {
    header: String,
    manual_prompt: String,
    scope: FilePickerScope,
    start_dir: Option<PathBuf>,
    picker_hint: Option<String>,
    manual_option_label: String,
    picker_option_label: String,
    wine_prefix_option_label: Option<String>,
}

impl PathInputBuilder {
    pub fn new() -> Self {
        let manual_icon = char::from(NerdFont::Edit);
        let picker_icon = char::from(NerdFont::FolderOpen);
        Self {
            header: format!(
                "{} Choose the path you want to use",
                char::from(NerdFont::Folder)
            ),
            manual_prompt: format!("{manual_icon} Enter the path:"),
            scope: FilePickerScope::FilesAndDirectories,
            start_dir: dirs::home_dir(),
            picker_hint: None,
            manual_option_label: format!("{manual_icon} Enter a specific path"),
            picker_option_label: format!("{picker_icon} Browse with the picker"),
            wine_prefix_option_label: None,
        }
    }

    pub fn header<S: Into<String>>(mut self, header: S) -> Self {
        self.header = header.into();
        self
    }

    pub fn manual_prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.manual_prompt = prompt.into();
        self
    }

    pub fn scope(mut self, scope: FilePickerScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn start_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.start_dir = Some(dir.into());
        self
    }

    pub fn picker_hint<S: Into<String>>(mut self, hint: S) -> Self {
        self.picker_hint = Some(hint.into());
        self
    }

    pub fn manual_option_label<S: Into<String>>(mut self, label: S) -> Self {
        self.manual_option_label = label.into();
        self
    }

    pub fn picker_option_label<S: Into<String>>(mut self, label: S) -> Self {
        self.picker_option_label = label.into();
        self
    }

    pub fn wine_prefix_option_label<S: Into<String>>(mut self, label: S) -> Self {
        self.wine_prefix_option_label = Some(label.into());
        self
    }

    fn wine_prefix_enabled(&self) -> bool {
        self.wine_prefix_option_label.is_some()
    }

    fn wine_prefix_label(&self) -> String {
        self.wine_prefix_option_label.clone().unwrap_or_else(|| {
            format!("{} Select a Wine prefix", char::from(NerdFont::Wine))
        })
    }

    pub fn choose(self) -> Result<PathInputSelection> {
        let mut options = vec![
            PathInputOption::new(self.manual_option_label.clone(), PathInputChoice::Manual),
            PathInputOption::new(self.picker_option_label.clone(), PathInputChoice::Picker),
        ];

        // Only add wine prefix option if explicitly configured
        if self.wine_prefix_enabled() {
            options.push(PathInputOption::new(
                self.wine_prefix_label(),
                PathInputChoice::WinePrefix,
            ));
        }

        loop {
            let selection = FzfWrapper::builder()
                .header(self.header.clone())
                .select(options.clone())?;

            match selection {
                FzfResult::Selected(option) => match option.choice {
                    PathInputChoice::Manual => {
                        let input = FzfWrapper::input(&self.manual_prompt)?;
                        let trimmed = input.trim().to_string();
                        if trimmed.is_empty() {
                            println!(
                                "{} No path entered. Please choose a path.",
                                char::from(NerdFont::Warning)
                            );
                            continue;
                        }
                        return Ok(PathInputSelection::Manual(trimmed));
                    }
                    PathInputChoice::Picker => {
                        let mut picker = MenuWrapper::file_picker().scope(self.scope);

                        if let Some(dir) = &self.start_dir {
                            picker = picker.start_dir(dir.clone());
                        }

                        if let Some(hint) = &self.picker_hint {
                            picker = picker.hint(hint.clone());
                        }

                        match picker.pick_one() {
                            Ok(Some(path)) => return Ok(PathInputSelection::Picker(path)),
                            Ok(None) => return Ok(PathInputSelection::Cancelled),
                            Err(err) => {
                                eprintln!("Failed to launch file picker: {err}");
                                continue;
                            }
                        }
                    }
                    PathInputChoice::WinePrefix => {
                        // For wine prefix selection, we'll use the picker but with a hint about wine prefixes
                        let mut picker = MenuWrapper::file_picker().scope(self.scope);

                        if let Some(dir) = &self.start_dir {
                            picker = picker.start_dir(dir.clone());
                        }

                        if let Some(hint) = &self.picker_hint {
                            picker = picker.hint(hint.clone());
                        }

                        match picker.pick_one() {
                            Ok(Some(path)) => return Ok(PathInputSelection::WinePrefix(path)),
                            Ok(None) => return Ok(PathInputSelection::Cancelled),
                            Err(err) => {
                                eprintln!("Failed to launch file picker: {err}");
                                continue;
                            }
                        }
                    }
                },
                FzfResult::Cancelled => return Ok(PathInputSelection::Cancelled),
                FzfResult::MultiSelected(_) => return Ok(PathInputSelection::Cancelled),
                FzfResult::Error(err) => return Err(anyhow!(err)),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum PathInputSelection {
    Manual(String),
    Picker(PathBuf),
    WinePrefix(PathBuf),
    Cancelled,
}

impl PathInputSelection {
    pub fn to_tilde_path(&self) -> Result<Option<TildePath>> {
        match self {
            PathInputSelection::Manual(input) => {
                if input.is_empty() {
                    return Ok(None);
                }
                Ok(Some(TildePath::from_str(input)?))
            }
            PathInputSelection::Picker(path) => Ok(Some(TildePath::new(path.clone()))),
            PathInputSelection::WinePrefix(path) => Ok(Some(TildePath::new(path.clone()))),
            PathInputSelection::Cancelled => Ok(None),
        }
    }

    pub fn to_path_buf(&self) -> Result<Option<PathBuf>> {
        Ok(self.to_tilde_path()?.map(|tilde| tilde.into_path_buf()))
    }

    pub fn to_tilde_string(&self) -> Result<Option<String>> {
        match self.to_tilde_path()? {
            Some(tilde) => Ok(Some(tilde.to_tilde_string()?)),
            None => Ok(None),
        }
    }
}
