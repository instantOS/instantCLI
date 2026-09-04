use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use anyhow::Result;

use super::file_picker::{FilePickerScope, MenuWrapper};
use super::fzf::{FzfSelectable, FzfWrapper, Header, HeaderBuilder};
use crate::common::TildePath;
use crate::preview::{PreviewId, preview_command};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{FzfPreview, PreviewBuilder};

/// Producer that discovers suggestion paths while the menu is open.
pub type SuggestionProducer = Arc<dyn Fn(SuggestionSink) + Send + Sync>;

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
    header: Header,
    manual_prompt: String,
    scope: FilePickerScope,
    start_dir: Option<PathBuf>,
    start_path: Option<PathBuf>,
    picker_hint: Option<String>,
    manual_option_label: String,
    picker_option_label: String,
    wine_prefix_option_label: Option<String>,
    suggested_paths: Vec<PathBuf>,
    streaming_suggestions: Option<SuggestionProducer>,
}

impl PathInputBuilder {
    pub fn new() -> Self {
        let manual_icon = char::from(NerdFont::Edit);
        let picker_icon = char::from(NerdFont::FolderOpen);
        Self {
            header: HeaderBuilder::new(NerdFont::Folder, "Choose the path you want to use").build(),
            manual_prompt: format!("{manual_icon} Enter the path:"),
            scope: FilePickerScope::FilesAndDirectories,
            start_dir: dirs::home_dir(),
            start_path: None,
            picker_hint: None,
            manual_option_label: format!("{manual_icon} Enter a specific path"),
            picker_option_label: format!("{picker_icon} Browse with the picker"),
            wine_prefix_option_label: None,
            suggested_paths: Vec::new(),
            streaming_suggestions: None,
        }
    }

    pub fn header<H: Into<Header>>(mut self, header: H) -> Self {
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

    pub fn start_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.start_path = Some(path.into());
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

    /// Register a producer that pushes additional suggestion paths while the
    /// menu is open.
    ///
    /// The menu renders immediately; suggestions appear as the producer
    /// discovers them, without blocking on filesystem scans.
    pub fn streaming_suggestions(mut self, producer: SuggestionProducer) -> Self {
        self.streaming_suggestions = Some(producer);
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

        if let Some(path) = &self.start_path {
            picker = picker.start_path(path.clone());
        }

        if let Some(hint) = &self.picker_hint {
            picker = picker.hint(hint.clone());
        }

        match picker.pick_one() {
            Ok(crate::menu_utils::DialogOutcome::Submitted(path)) => Ok(Some(path)),
            Ok(crate::menu_utils::DialogOutcome::Cancelled) => Ok(None),
            Err(err) => {
                eprintln!("Failed to launch file picker: {err:#}");
                Ok(None) // Signal to retry by returning None
            }
        }
    }

    fn run_picker_at(&self, path: &Path) -> Result<Option<PathBuf>> {
        let mut picker = MenuWrapper::file_picker().scope(self.scope);

        if path.is_dir() {
            picker = picker.start_dir(path.to_path_buf());
        } else {
            picker = picker.start_path(path.to_path_buf());

            if let Some(parent) = path.parent() {
                picker = picker.start_dir(parent.to_path_buf());
            }
        }

        if let Some(hint) = &self.picker_hint {
            picker = picker.hint(hint.clone());
        }

        match picker.pick_one() {
            Ok(crate::menu_utils::DialogOutcome::Submitted(path)) => Ok(Some(path)),
            Ok(crate::menu_utils::DialogOutcome::Cancelled) => Ok(None),
            Err(err) => {
                eprintln!("Failed to launch file picker: {err:#}");
                Ok(None)
            }
        }
    }

    fn build_options(&self) -> Vec<PathInputOption> {
        let mut options = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for path in &self.suggested_paths {
            let (_, key) = normalize_suggested_path(path);
            if !seen.insert(key) {
                continue;
            }

            options.push(suggestion_option(path));
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

    fn should_open_picker_for_suggestion(&self, path: &Path) -> bool {
        self.scope == FilePickerScope::Files && path.is_dir()
    }

    fn prompt_manual_path(&self) -> Result<ManualPathOutcome> {
        match FzfWrapper::builder()
            .prompt(&self.manual_prompt)
            .input()
            .input_dialog()?
        {
            crate::menu_utils::DialogOutcome::Submitted(input) => {
                let trimmed = input.trim().to_string();
                if trimmed.is_empty() {
                    println!(
                        "{} No path entered. Please choose a path.",
                        char::from(NerdFont::Warning)
                    );
                    return Ok(ManualPathOutcome::Retry);
                }

                Ok(ManualPathOutcome::Submitted(trimmed))
            }
            crate::menu_utils::DialogOutcome::Cancelled => Ok(ManualPathOutcome::Cancelled),
        }
    }

    pub fn choose(self) -> Result<PathInputSelection> {
        let options = self.build_options();

        loop {
            let selection = match &self.streaming_suggestions {
                Some(producer) => {
                    self.select_with_streaming_suggestions(&options, Arc::clone(producer))?
                }
                None => FzfWrapper::builder()
                    .header(self.header.clone())
                    .select_one(options.clone())?,
            };

            match selection {
                crate::menu_utils::DialogOutcome::Submitted(option) => match option.choice {
                    PathInputChoice::Manual => match self.prompt_manual_path()? {
                        ManualPathOutcome::Submitted(input) => {
                            return Ok(PathInputSelection::Manual(input));
                        }
                        ManualPathOutcome::Retry => continue,
                        ManualPathOutcome::Cancelled => {
                            continue;
                        }
                    },
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
                        if self.should_open_picker_for_suggestion(&path) {
                            match self.run_picker_at(&path)? {
                                Some(selected) => return Ok(PathInputSelection::Picker(selected)),
                                None => continue,
                            }
                        }

                        return Ok(PathInputSelection::Picker(path));
                    }
                },
                crate::menu_utils::DialogOutcome::Cancelled => {
                    return Ok(PathInputSelection::Cancelled);
                }
            }
        }
    }

    /// Open the menu with `options` shown immediately, then stream further
    /// suggestions in from `producer` as they are discovered.
    fn select_with_streaming_suggestions(
        &self,
        options: &[PathInputOption],
        producer: SuggestionProducer,
    ) -> Result<crate::menu_utils::DialogOutcome<PathInputOption>> {
        let (tx, rx) = crossbeam_channel::unbounded();
        thread::spawn(move || producer(SuggestionSink { tx }));

        // Jump to the first streamed suggestion once fzf finished loading its
        // input; before that the cursor rests on the first static option.
        Ok(
            match FzfWrapper::builder()
                .header(self.header.clone())
                .args([
                    "--bind".to_string(),
                    format!("load:pos({})", options.len() + 1),
                ])
                .select_streaming(options.to_vec(), rx)?
            {
                crate::menu_utils::FzfResult::Selected(option) => {
                    crate::menu_utils::DialogOutcome::Submitted(option)
                }
                // Single-select menu: multi-selection cannot occur.
                crate::menu_utils::FzfResult::MultiSelected(_) => {
                    crate::menu_utils::DialogOutcome::Cancelled
                }
                crate::menu_utils::FzfResult::Cancelled => {
                    crate::menu_utils::DialogOutcome::Cancelled
                }
            },
        )
    }
}

enum ManualPathOutcome {
    Submitted(String),
    Retry,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathInputSelection {
    Manual(String),
    Picker(PathBuf),
    WinePrefix(PathBuf),
    Cancelled,
}

fn normalize_suggested_path(path: &Path) -> (PathBuf, String) {
    if let Ok(canonical) = path.canonicalize()
        && canonical.exists()
    {
        let key = canonical.to_string_lossy().to_string();
        return (canonical, key);
    }

    (path.to_path_buf(), path.to_string_lossy().to_string())
}

/// Build a selectable suggestion entry for `path`.
fn suggestion_option(path: &Path) -> PathInputOption {
    let (path, _) = normalize_suggested_path(path);
    let preview = preview_suggestion(&path);
    PathInputOption::new_with_preview(
        format_suggested_label(&path),
        PathInputChoice::Suggestion(path),
        preview,
    )
}

/// Handle producers use to push suggestion paths into an open menu.
///
/// Pushes are best-effort: once the menu has closed, pushes are silently
/// dropped so producers stop as soon as they notice.
pub struct SuggestionSink {
    tx: crossbeam_channel::Sender<PathInputOption>,
}

impl SuggestionSink {
    /// Suggest `path` as a selectable entry in the open menu.
    pub fn push(&self, path: PathBuf) {
        let _ = self.tx.send(suggestion_option(&path));
    }
}

fn format_suggested_label(path: &Path) -> String {
    let icon = if path.is_dir() {
        char::from(NerdFont::Folder)
    } else {
        char::from(NerdFont::File)
    };
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
    FzfPreview::Command(preview_command(PreviewId::FileSuggestion))
}

impl PathInputSelection {
    pub fn to_tilde_path(&self) -> Result<Option<TildePath>> {
        match self {
            PathInputSelection::Manual(input) => {
                if input.is_empty() {
                    return Ok(None);
                }
                Ok(Some(TildePath::from_str(input)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_scope_directory_suggestion_opens_picker() {
        let builder = PathInputBuilder::new().scope(FilePickerScope::Files);

        assert!(builder.should_open_picker_for_suggestion(Path::new("/tmp")));
    }

    #[test]
    fn file_scope_file_suggestion_returns_directly() {
        let builder = PathInputBuilder::new().scope(FilePickerScope::Files);

        assert!(!builder.should_open_picker_for_suggestion(Path::new("/tmp/game.exe")));
    }

    #[test]
    fn directory_scope_directory_suggestion_returns_directly() {
        let builder = PathInputBuilder::new().scope(FilePickerScope::Directories);

        assert!(!builder.should_open_picker_for_suggestion(Path::new("/tmp")));
    }

    #[test]
    fn suggestion_options_distinguish_directories_from_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("Games");
        std::fs::create_dir_all(&dir).unwrap();
        let file = temp.path().join("Game.exe");
        std::fs::write(&file, b"").unwrap();

        let dir_label = suggestion_option(&dir).fzf_display_text();
        let file_label = suggestion_option(&file).fzf_display_text();

        assert!(dir_label.starts_with(char::from(NerdFont::Folder)));
        assert!(file_label.starts_with(char::from(NerdFont::File)));
        assert_ne!(dir_label, file_label);
    }

    #[test]
    fn streaming_suggestions_menu_wires_producer_and_cancels_cleanly() {
        use crate::menu_utils::MockQueue;

        let temp = tempfile::tempdir().unwrap();
        let game_exe = temp.path().join("Game.exe");
        std::fs::write(&game_exe, b"").unwrap();

        let builder = PathInputBuilder::new().streaming_suggestions(Arc::new(move |sink| {
            sink.push(game_exe.clone());
        }));

        let _guard = MockQueue::new().cancel_selection().guard();
        let selection = builder.choose().unwrap();
        assert_eq!(selection, PathInputSelection::Cancelled);
    }
}
