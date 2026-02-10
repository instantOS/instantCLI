use anyhow::{Context, Result, anyhow};
use dirs::cache_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::{Builder as TempFileBuilder, NamedTempFile};
use which::which;

use super::fzf::FzfWrapper;

const YAZI_INIT_LUA: &str = include_str!("yazi_init.lua");
const YAZI_CACHE_SUBDIR: &str = "ins/menu/yazi";

fn yazi_config_dir() -> PathBuf {
    let mut base = cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    for segment in YAZI_CACHE_SUBDIR.split('/') {
        base.push(segment);
    }
    base
}

fn ensure_yazi_config(show_hidden: bool) -> Result<PathBuf> {
    let dir = yazi_config_dir();
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "Failed to create Yazi config directory at {}",
            dir.display()
        )
    })?;

    let init_path = dir.join("init.lua");
    write_if_changed(&init_path, YAZI_INIT_LUA)?;

    let config_path = dir.join("yazi.toml");
    let config_content = format!(
        "[mgr]\nshow_hidden = {}\nsort_by = \"natural\"\n",
        if show_hidden { "true" } else { "false" }
    );
    write_if_changed(&config_path, &config_content)?;

    Ok(dir)
}

fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    let should_write = match fs::read_to_string(path) {
        Ok(existing) => existing != contents,
        Err(_) => true,
    };

    if should_write {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to prepare directory for {}", parent.display()))?;
        }

        fs::write(path, contents).with_context(|| format!("Failed to write {}", path.display()))?;
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum FilePickerScope {
    #[default]
    Files,
    Directories,
    FilesAndDirectories,
}

impl FilePickerScope {
    pub fn as_env_value(&self) -> &'static str {
        match self {
            FilePickerScope::Files => "files",
            FilePickerScope::Directories => "directories",
            FilePickerScope::FilesAndDirectories => "both",
        }
    }
}

#[derive(Debug)]
pub enum FilePickerResult {
    Selected(PathBuf),
    MultiSelected(Vec<PathBuf>),
    Cancelled,
}

pub struct FilePickerBuilder {
    start_dir: Option<PathBuf>,
    scope: FilePickerScope,
    multi: bool,
    custom_hint: Option<String>,
    show_hidden: bool,
}

impl FilePickerBuilder {
    pub fn new() -> Self {
        Self {
            start_dir: None,
            scope: FilePickerScope::Files,
            multi: false,
            custom_hint: None,
            show_hidden: false,
        }
    }

    pub fn start_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.start_dir = Some(dir.into());
        self
    }

    pub fn scope(mut self, scope: FilePickerScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn multi(mut self, multi: bool) -> Self {
        self.multi = multi;
        self
    }

    pub fn hint<S: Into<String>>(mut self, hint: S) -> Self {
        self.custom_hint = Some(hint.into());
        self
    }

    pub fn show_hidden(mut self, show: bool) -> Self {
        self.show_hidden = show;
        self
    }

    pub fn pick(self) -> Result<FilePickerResult> {
        self.run_yazi()
    }

    pub fn pick_one(self) -> Result<Option<PathBuf>> {
        match self.pick()? {
            FilePickerResult::Selected(path) => Ok(Some(path)),
            FilePickerResult::MultiSelected(mut paths) => Ok(paths.pop()),
            FilePickerResult::Cancelled => Ok(None),
        }
    }

    fn run_yazi(self) -> Result<FilePickerResult> {
        let yazi_path = Self::resolve_yazi_path()?;
        let config_dir = ensure_yazi_config(self.show_hidden)?;
        self.pick_with_yazi(&yazi_path, &config_dir)
    }

    fn resolve_yazi_path() -> Result<PathBuf> {
        which("yazi")
            .context("`yazi` command was not found. Install it to use the menu file picker.")
    }

    fn pick_with_yazi(&self, yazi_path: &Path, config_dir: &Path) -> Result<FilePickerResult> {
        let mut preselect: Option<PathBuf> = None;

        loop {
            let chooser_file = Self::create_chooser_file()?;
            let chooser_path = chooser_file.path().to_path_buf();

            let status =
                self.launch_yazi(yazi_path, config_dir, &chooser_path, preselect.as_deref())?;

            if !status.success() {
                return Ok(FilePickerResult::Cancelled);
            }

            let selections = Self::read_selections(&chooser_path);
            drop(chooser_file);

            if selections.is_empty() {
                return Ok(FilePickerResult::Cancelled);
            }

            let (selections, invalid_entries) = self.filter_by_scope(selections);

            if let Some(next_preselect) = self.handle_directory_invalid_entries(&invalid_entries)? {
                preselect = Some(next_preselect);
                continue;
            }

            if selections.is_empty() {
                if !invalid_entries.is_empty() {
                    let requested = self.selection_kind();
                    return Err(anyhow!(
                        "No {} selected. Ensure you press Enter on a valid {}.",
                        requested,
                        requested
                    ));
                }
                return Ok(FilePickerResult::Cancelled);
            }

            return Ok(self.finalize_selection(selections));
        }
    }

    fn create_chooser_file() -> Result<NamedTempFile> {
        TempFileBuilder::new()
            .prefix("ins-menu-picker-")
            .suffix(".txt")
            .tempfile()
            .context("Failed to create temporary chooser file")
    }

    fn read_selections(chooser_path: &Path) -> Vec<PathBuf> {
        fs::read_to_string(chooser_path)
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect()
    }

    fn filter_by_scope(&self, selections: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut valid = Vec::new();
        let mut invalid = Vec::new();

        for path in selections {
            match self.scope {
                FilePickerScope::Files => {
                    if path.is_file() {
                        valid.push(path);
                    } else {
                        invalid.push(path);
                    }
                }
                FilePickerScope::Directories => {
                    if path.is_dir() {
                        valid.push(path);
                    } else {
                        invalid.push(path);
                    }
                }
                FilePickerScope::FilesAndDirectories => valid.push(path),
            }
        }

        (valid, invalid)
    }

    fn handle_directory_invalid_entries(
        &self,
        invalid_entries: &[PathBuf],
    ) -> Result<Option<PathBuf>> {
        if self.scope != FilePickerScope::Directories || invalid_entries.is_empty() {
            return Ok(None);
        }

        // invalid_entries is guaranteed to be non-empty here due to the guard above
        let first_invalid = invalid_entries.first().unwrap();
        let message = format!(
            "`{}` is a file.\n\nPlease choose a directory instead.",
            first_invalid.display()
        );
        FzfWrapper::message(&message)?;

        Ok(invalid_entries.first().cloned())
    }

    fn selection_kind(&self) -> &'static str {
        match self.scope {
            FilePickerScope::Files => "file",
            FilePickerScope::Directories => "directory",
            FilePickerScope::FilesAndDirectories => "entry",
        }
    }

    fn finalize_selection(&self, mut selections: Vec<PathBuf>) -> FilePickerResult {
        for path in &mut selections {
            if let Ok(canonical) = fs::canonicalize(path.as_path()) {
                *path = canonical;
            }
        }

        if self.multi {
            FilePickerResult::MultiSelected(selections)
        } else {
            FilePickerResult::Selected(selections.into_iter().next().unwrap())
        }
    }

    fn launch_yazi(
        &self,
        yazi_path: &Path,
        config_dir: &Path,
        chooser_path: &Path,
        initial_selection: Option<&Path>,
    ) -> Result<std::process::ExitStatus> {
        let mut cmd = Command::new(yazi_path);
        cmd.arg("--chooser-file")
            .arg(chooser_path)
            .env("YAZI_CONFIG_HOME", config_dir)
            .env("INS_MENU_PICKER_MULTI", if self.multi { "1" } else { "0" })
            .env("INS_MENU_PICKER_SCOPE", self.scope.as_env_value());

        if let Some(ref hint) = self.custom_hint {
            cmd.env("INS_MENU_PICKER_HINT", hint);
        }

        let mut current_dir = self.start_dir.clone();

        if let Some(selection) = initial_selection
            && current_dir.is_none()
        {
            current_dir = selection
                .is_dir()
                .then(|| selection.to_path_buf())
                .or_else(|| selection.parent().map(|parent| parent.to_path_buf()));
        }

        let launch_target = if let Some(selection) = initial_selection {
            Some(selection.to_path_buf())
        } else {
            self.start_dir.clone()
        };

        if let Some(target) = &launch_target {
            cmd.arg(target);
        }

        if let Some(dir) = current_dir {
            cmd.current_dir(dir);
        }

        let mut child = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to launch Yazi file picker")?;

        let pid = child.id();
        let _ = crate::menu::server::register_menu_process(pid);
        let status = child.wait().context("Failed to wait for Yazi process")?;
        crate::menu::server::unregister_menu_process(pid);

        Ok(status)
    }
}

pub struct MenuWrapper;

impl MenuWrapper {
    pub fn file_picker() -> FilePickerBuilder {
        FilePickerBuilder::new()
    }
}
