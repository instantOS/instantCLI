use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{
    ConfirmResult, FilePickerScope, FzfResult, FzfSelectable, FzfWrapper, PathInputBuilder,
    PathInputSelection,
};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(super) struct FileSelectionPrompt {
    pub header: String,
    pub picker_hint: String,
    pub manual_option_label: String,
    pub picker_option_label: String,
    pub suggested_paths: Vec<PathBuf>,
}

pub(super) struct AppImageSelectionPrompt {
    pub header: String,
    pub picker_hint: String,
    pub missing_message: String,
}

impl FileSelectionPrompt {
    pub(super) fn game_file(header: String, picker_hint: String) -> Self {
        Self {
            header,
            picker_hint,
            manual_option_label: format!("{} Type game file path", char::from(NerdFont::Edit)),
            picker_option_label: format!(
                "{} Browse for game file",
                char::from(NerdFont::FolderOpen)
            ),
            suggested_paths: Vec::new(),
        }
    }

    pub(super) fn new(
        header: String,
        picker_hint: String,
        manual_option_label: String,
        picker_option_label: String,
    ) -> Self {
        Self {
            header,
            picker_hint,
            manual_option_label,
            picker_option_label,
            suggested_paths: Vec::new(),
        }
    }

    pub(super) fn suggested_paths<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.suggested_paths = paths.into_iter().map(Into::into).collect();
        self
    }
}

impl AppImageSelectionPrompt {
    pub(super) fn new(header: String, picker_hint: String, missing_message: String) -> Self {
        Self {
            header,
            picker_hint,
            missing_message,
        }
    }
}

pub(super) fn select_file_with_validation<F>(
    prompt: FileSelectionPrompt,
    validate: F,
) -> Result<Option<PathBuf>>
where
    F: Fn(&Path) -> Result<(), String>,
{
    let selection = PathInputBuilder::new()
        .header(prompt.header)
        .scope(FilePickerScope::Files)
        .picker_hint(prompt.picker_hint)
        .manual_option_label(prompt.manual_option_label)
        .picker_option_label(prompt.picker_option_label)
        .suggested_paths(prompt.suggested_paths)
        .choose()?;

    match selection {
        PathInputSelection::Manual(input) => {
            let path = PathBuf::from(shellexpand::tilde(&input).into_owned());
            if let Err(e) = validate(&path) {
                FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        PathInputSelection::Picker(path) => {
            if let Err(e) = validate(&path) {
                FzfWrapper::message(&format!("{} {}", char::from(NerdFont::CrossCircle), e))?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        PathInputSelection::WinePrefix(_) => Ok(None),
        PathInputSelection::Cancelled => Ok(None),
    }
}

pub(super) fn ask_fullscreen() -> Result<bool> {
    match FzfWrapper::confirm(&format!(
        "{} Run in fullscreen mode?",
        char::from(NerdFont::Fullscreen)
    ))? {
        ConfirmResult::Yes => Ok(true),
        _ => Ok(false),
    }
}

pub(super) fn select_detected_appimage(
    paths: &[PathBuf],
    icon: NerdFont,
    product_name: &str,
) -> Result<Option<PathBuf>> {
    match paths {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => {
            let header = format!("Select {product_name} AppImage");
            let preview_title = format!("Detected {product_name} AppImage");
            let preview_text = format!("Multiple {product_name} AppImages were found.");

            #[derive(Clone)]
            struct AppImageItem {
                path: PathBuf,
                icon: NerdFont,
                preview_title: String,
                preview_text: String,
            }

            impl FzfSelectable for AppImageItem {
                fn fzf_display_text(&self) -> String {
                    format!("{} {}", char::from(self.icon), self.path.display())
                }

                fn fzf_key(&self) -> String {
                    self.path.to_string_lossy().into_owned()
                }

                fn fzf_preview(&self) -> FzfPreview {
                    PreviewBuilder::new()
                        .header(self.icon, &self.preview_title)
                        .text(&self.preview_text)
                        .blank()
                        .field("Path", &self.path.display().to_string())
                        .build()
                }
            }

            let items: Vec<AppImageItem> = paths
                .iter()
                .cloned()
                .map(|path| AppImageItem {
                    path,
                    icon,
                    preview_title: preview_title.to_string(),
                    preview_text: preview_text.to_string(),
                })
                .collect();

            match FzfWrapper::builder()
                .header(format!("{} {}", char::from(icon), header))
                .prompt(product_name)
                .select(items)?
            {
                FzfResult::Selected(item) => Ok(Some(item.path)),
                FzfResult::Cancelled => Ok(None),
                _ => Ok(None),
            }
        }
    }
}

pub(super) fn select_appimage_manually(prompt: AppImageSelectionPrompt) -> Result<Option<PathBuf>> {
    let selection = PathInputBuilder::new()
        .header(prompt.header)
        .scope(FilePickerScope::Files)
        .picker_hint(prompt.picker_hint)
        .manual_option_label(format!("{} Type AppImage path", char::from(NerdFont::Edit)))
        .picker_option_label(format!(
            "{} Browse for AppImage",
            char::from(NerdFont::FolderOpen)
        ))
        .choose()?;

    match selection {
        PathInputSelection::Manual(input) => {
            let path = PathBuf::from(shellexpand::tilde(&input).into_owned());
            if !path.exists() {
                FzfWrapper::message(&format!(
                    "{} {}",
                    char::from(NerdFont::CrossCircle),
                    prompt
                        .missing_message
                        .replace("{}", &path.display().to_string())
                ))?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        PathInputSelection::Picker(path) => {
            if !path.exists() {
                FzfWrapper::message(&format!(
                    "{} {}",
                    char::from(NerdFont::CrossCircle),
                    prompt
                        .missing_message
                        .replace("{}", &path.display().to_string())
                ))?;
                return Ok(None);
            }
            Ok(Some(path))
        }
        PathInputSelection::WinePrefix(_) => Ok(None),
        PathInputSelection::Cancelled => Ok(None),
    }
}

pub(super) fn confirm_command(command: &impl std::fmt::Display) -> Result<bool> {
    let message = format!(
        "{} Generated Launch Command:\n\n{}\n\nUse this command?",
        char::from(NerdFont::Rocket),
        command
    );

    match FzfWrapper::confirm(&message)? {
        ConfirmResult::Yes => Ok(true),
        _ => Ok(false),
    }
}

pub(super) fn confirm_value<T>(value: T) -> Result<Option<T>>
where
    T: std::fmt::Display,
{
    if confirm_command(&value)? {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}
