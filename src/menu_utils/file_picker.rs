use anyhow::{Context, Result, anyhow};
use dirs::cache_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::Builder as TempFileBuilder;
use which::which;

const YAZI_INIT_LUA: &str = include_str!("yazi_init.lua");
const YAZI_CACHE_SUBDIR: &str = "ins/menu/yazi";

fn yazi_config_dir() -> PathBuf {
    let mut base = cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    for segment in YAZI_CACHE_SUBDIR.split('/') {
        base.push(segment);
    }
    base
}

fn ensure_yazi_config() -> Result<PathBuf> {
    let dir = yazi_config_dir();
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "Failed to create Yazi config directory at {}",
            dir.display()
        )
    })?;

    let init_path = dir.join("init.lua");
    write_if_changed(&init_path, YAZI_INIT_LUA)?;

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
}

impl FilePickerBuilder {
    pub fn new() -> Self {
        Self {
            start_dir: None,
            scope: FilePickerScope::Files,
            multi: false,
            custom_hint: None,
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
        let yazi_path = which("yazi")
            .context("`yazi` command was not found. Install it to use the menu file picker.")?;

        let config_dir = ensure_yazi_config()?;

        let chooser_file = TempFileBuilder::new()
            .prefix("ins-menu-picker-")
            .suffix(".txt")
            .tempfile()
            .context("Failed to create temporary chooser file")?;
        let chooser_path = chooser_file.path().to_path_buf();

        if !self
            .launch_yazi(&yazi_path, &config_dir, &chooser_path)?
            .success()
        {
            return Ok(FilePickerResult::Cancelled);
        }

        let mut selections: Vec<PathBuf> = fs::read_to_string(&chooser_path)
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect();

        drop(chooser_file);

        if selections.is_empty() {
            return Ok(FilePickerResult::Cancelled);
        }

        let mut invalid_entries = Vec::new();
        selections.retain(|path| match self.scope {
            FilePickerScope::Files => {
                if path.is_file() {
                    true
                } else {
                    invalid_entries.push(path.clone());
                    false
                }
            }
            FilePickerScope::Directories => {
                if path.is_dir() {
                    true
                } else {
                    invalid_entries.push(path.clone());
                    false
                }
            }
            FilePickerScope::FilesAndDirectories => true,
        });

        if selections.is_empty() {
            if !invalid_entries.is_empty() {
                let requested = match self.scope {
                    FilePickerScope::Files => "file",
                    FilePickerScope::Directories => "directory",
                    FilePickerScope::FilesAndDirectories => "entry",
                };
                return Err(anyhow!(
                    "No {} selected. Ensure you press Enter on a valid {}.",
                    requested,
                    requested
                ));
            }
            return Ok(FilePickerResult::Cancelled);
        }

        for path in &mut selections {
            if let Ok(canonical) = fs::canonicalize(path.as_path()) {
                *path = canonical;
            }
        }

        if self.multi {
            Ok(FilePickerResult::MultiSelected(selections))
        } else {
            Ok(FilePickerResult::Selected(
                selections.into_iter().next().unwrap(),
            ))
        }
    }

    fn launch_yazi(
        &self,
        yazi_path: &Path,
        config_dir: &Path,
        chooser_path: &Path,
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

        if let Some(dir) = &self.start_dir {
            cmd.arg(dir);
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

    pub fn pick_file() -> Result<Option<PathBuf>> {
        FilePickerBuilder::new().pick_one()
    }

    pub fn pick_files() -> Result<Vec<PathBuf>> {
        match FilePickerBuilder::new().multi(true).pick()? {
            FilePickerResult::MultiSelected(paths) => Ok(paths),
            FilePickerResult::Selected(path) => Ok(vec![path]),
            FilePickerResult::Cancelled => Ok(vec![]),
        }
    }
}
