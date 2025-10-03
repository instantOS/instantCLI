use std::path::PathBuf;

use anyhow::{Result, anyhow};

use super::file_picker::{FilePickerScope, MenuWrapper};
use super::fzf::{FzfResult, FzfSelectable, FzfWrapper};
use crate::dot::path_serde::TildePath;

#[derive(Debug, Clone)]
enum PathInputChoice {
    Manual,
    Picker,
}

#[derive(Debug, Clone)]
struct PathInputOption {
    label: String,
    choice: PathInputChoice,
}

impl PathInputOption {
    fn manual() -> Self {
        Self {
            label: "Type or paste a path manually".to_string(),
            choice: PathInputChoice::Manual,
        }
    }

    fn picker() -> Self {
        Self {
            label: "Use the interactive file picker".to_string(),
            choice: PathInputChoice::Picker,
        }
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
}

impl PathInputBuilder {
    pub fn new() -> Self {
        Self {
            header: "How would you like to provide the path?".to_string(),
            manual_prompt: "Enter path:".to_string(),
            scope: FilePickerScope::FilesAndDirectories,
            start_dir: dirs::home_dir(),
            picker_hint: None,
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

    pub fn choose(self) -> Result<PathInputSelection> {
        let options = vec![PathInputOption::manual(), PathInputOption::picker()];

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
                            return Ok(PathInputSelection::Cancelled);
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
