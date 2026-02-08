use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow};

use super::file_picker::{FilePickerScope, MenuWrapper};
use super::fzf::{FzfResult, FzfSelectable, FzfWrapper};
use crate::common::TildePath;
use crate::preview::{PreviewId, preview_command};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

/// A function that generates a custom preview for a suggested path
pub type SuggestionPreviewFn = Arc<dyn Fn(&Path) -> FzfPreview + Send + Sync>;

#[derive(Debug, Clone)]
enum PathInputChoice {
    Manual,
    Picker,
    WinePrefix,
    Suggestion(PathBuf),
}

#[derive(Clone)]
struct PathInputOption {
    label: String,
    choice: PathInputChoice,
    custom_preview: Option<FzfPreview>,
}

impl PathInputOption {
    fn new(label: String, choice: PathInputChoice) -> Self {
        Self {
            label,
            choice,
            custom_preview: None,
        }
    }

    fn new_with_preview(label: String, choice: PathInputChoice, preview: FzfPreview) -> Self {
        Self {
            label,
            choice,
            custom_preview: Some(preview),
        }
    }
}

impl FzfSelectable for PathInputOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_key(&self) -> String {
        match &self.choice {
            PathInputChoice::Suggestion(path) => path.to_string_lossy().to_string(),
            _ => self.label.clone(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        if let Some(preview) = &self.custom_preview {
            return preview.clone();
        }
        match &self.choice {
            PathInputChoice::Manual => preview_manual(),
            PathInputChoice::Picker => preview_picker(),
            PathInputChoice::WinePrefix => preview_wine_prefix(),
            PathInputChoice::Suggestion(path) => preview_suggestion(path),
        }
    }
}

#[derive(Clone)]
pub struct PathInputBuilder {
    header: String,
    manual_prompt: String,
    scope: FilePickerScope,
    start_dir: Option<PathBuf>,
    picker_hint: Option<String>,
    manual_option_label: String,
    picker_option_label: String,
    wine_prefix_option_label: Option<String>,
    suggested_paths: Vec<PathBuf>,
    suggestion_preview_fn: Option<SuggestionPreviewFn>,
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
            suggested_paths: Vec::new(),
            suggestion_preview_fn: None,
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

    pub fn suggested_paths<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.suggested_paths = paths.into_iter().map(Into::into).collect();
        self
    }

    /// Set a custom preview function for suggested paths.
    /// When set, this function is called instead of the default preview_suggestion.
    pub fn suggestion_preview<F>(mut self, f: F) -> Self
    where
        F: Fn(&Path) -> FzfPreview + Send + Sync + 'static,
    {
        self.suggestion_preview_fn = Some(Arc::new(f));
        self
    }

    fn wine_prefix_enabled(&self) -> bool {
        self.wine_prefix_option_label.is_some()
    }

    fn wine_prefix_label(&self) -> String {
        self.wine_prefix_option_label
            .clone()
            .unwrap_or_else(|| format!("{} Select a Wine prefix", char::from(NerdFont::Wine)))
    }

    fn run_picker(&self) -> Result<Option<PathBuf>> {
        let mut picker = MenuWrapper::file_picker().scope(self.scope);

        if let Some(dir) = &self.start_dir {
            picker = picker.start_dir(dir.clone());
        }

        if let Some(hint) = &self.picker_hint {
            picker = picker.hint(hint.clone());
        }

        match picker.pick_one() {
            Ok(path) => Ok(path),
            Err(err) => {
                eprintln!("Failed to launch file picker: {err}");
                Ok(None) // Signal to retry by returning None
            }
        }
    }

    fn suggestion_preview_for(&self, path: &Path) -> FzfPreview {
        match &self.suggestion_preview_fn {
            Some(f) => f(path),
            None => preview_suggestion(path),
        }
    }

    fn normalize_suggested_path(&self, path: &Path) -> (PathBuf, String) {
        if let Ok(canonical) = path.canonicalize()
            && canonical.exists()
        {
            let key = canonical.to_string_lossy().to_string();
            return (canonical, key);
        }

        (path.to_path_buf(), path.to_string_lossy().to_string())
    }

    fn build_options(&self) -> Vec<PathInputOption> {
        let mut options = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for path in &self.suggested_paths {
            let (path, key) = self.normalize_suggested_path(path);
            if !seen.insert(key) {
                continue;
            }

            let preview = self.suggestion_preview_for(&path);
            options.push(PathInputOption::new_with_preview(
                format_suggested_label(&path),
                PathInputChoice::Suggestion(path),
                preview,
            ));
        }

        options.push(PathInputOption::new(
            self.manual_option_label.clone(),
            PathInputChoice::Manual,
        ));
        options.push(PathInputOption::new(
            self.picker_option_label.clone(),
            PathInputChoice::Picker,
        ));

        // Only add wine prefix option if explicitly configured
        if self.wine_prefix_enabled() {
            options.push(PathInputOption::new(
                self.wine_prefix_label(),
                PathInputChoice::WinePrefix,
            ));
        }

        options
    }

    fn prompt_manual_path(&self) -> Result<Option<String>> {
        let input = FzfWrapper::input(&self.manual_prompt)?;
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            println!(
                "{} No path entered. Please choose a path.",
                char::from(NerdFont::Warning)
            );
            return Ok(None);
        }

        Ok(Some(trimmed))
    }

    pub fn choose(self) -> Result<PathInputSelection> {
        let options = self.build_options();

        loop {
            let selection = FzfWrapper::builder()
                .header(self.header.clone())
                .select(options.clone())?;

            match selection {
                FzfResult::Selected(option) => match option.choice {
                    PathInputChoice::Manual => {
                        if let Some(input) = self.prompt_manual_path()? {
                            return Ok(PathInputSelection::Manual(input));
                        }

                        continue;
                    }
                    PathInputChoice::Picker => {
                        match self.run_picker()? {
                            Some(path) => return Ok(PathInputSelection::Picker(path)),
                            None => continue, // Error occurred, retry
                        }
                    }
                    PathInputChoice::WinePrefix => {
                        match self.run_picker()? {
                            Some(path) => return Ok(PathInputSelection::WinePrefix(path)),
                            None => continue, // Error occurred, retry
                        }
                    }
                    PathInputChoice::Suggestion(path) => {
                        return Ok(PathInputSelection::Picker(path));
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

fn format_suggested_label(path: &Path) -> String {
    let icon = char::from(NerdFont::Star);
    let display = path.to_string_lossy();
    let short = if display.len() > 80 {
        format!("{}...", &display[..79])
    } else {
        display.to_string()
    };
    format!("{icon} {short}")
}

fn preview_manual() -> FzfPreview {
    PreviewBuilder::new()
        .header(NerdFont::Edit, "Enter a path")
        .text("Type a path manually in the next prompt.")
        .blank()
        .text("Tips:")
        .bullet("Use ~ for your home directory")
        .bullet("Paste absolute paths")
        .bullet("Trailing / treats input as a folder")
        .build()
}

fn preview_picker() -> FzfPreview {
    PreviewBuilder::new()
        .header(NerdFont::FolderOpen, "Browse with picker")
        .text("Launch the file picker to browse the filesystem.")
        .blank()
        .text("Useful when you want to visually select a path.")
        .build()
}

fn preview_wine_prefix() -> FzfPreview {
    PreviewBuilder::new()
        .header(NerdFont::Wine, "Select Wine prefix")
        .text("Pick a Wine prefix directory for Windows paths.")
        .blank()
        .text("Choose the root of the prefix (usually ends with /drive_c).")
        .build()
}

fn preview_suggestion(_path: &Path) -> FzfPreview {
    // Use async command-based preview for rich file type detection
    // The path is passed as the fzf key ($1) to the preview command
    FzfPreview::Command(preview_command(PreviewId::FileSuggestion))
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
}
