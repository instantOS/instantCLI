//! Delete flow for subdirectory actions.

use anyhow::Result;

use crate::dot::config::DotfileConfig;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Delete confirmation choice
#[derive(Clone)]
enum DeleteChoice {
    KeepFiles,
    DeleteFiles,
    Cancel,
}

impl std::fmt::Display for DeleteChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeleteChoice::KeepFiles => write!(f, "keep"),
            DeleteChoice::DeleteFiles => write!(f, "delete"),
            DeleteChoice::Cancel => write!(f, "cancel"),
        }
    }
}

impl FzfSelectable for DeleteChoice {
    fn fzf_display_text(&self) -> String {
        match self {
            DeleteChoice::KeepFiles => format!(
                "{} Keep files (remove from config only)",
                format_icon_colored(NerdFont::File, colors::YELLOW)
            ),
            DeleteChoice::DeleteFiles => format!(
                "{} Delete files from disk",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            DeleteChoice::Cancel => format!("{} Cancel", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        self.to_string()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;
        match self {
            DeleteChoice::KeepFiles => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::File, "Keep Files")
                    .blank()
                    .text("Remove this directory from the repository config,")
                    .text("but keep the files on disk.")
                    .blank()
                    .text("The directory will no longer be recognized as a")
                    .text("dotfile source, but you can add it back later.")
                    .build_string(),
            ),
            DeleteChoice::DeleteFiles => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Trash, "Delete Files")
                    .blank()
                    .line(
                        colors::RED,
                        Some(NerdFont::Warning),
                        "This will permanently delete:",
                    )
                    .bullet("The directory and all its contents")
                    .bullet("Any dotfiles stored in this location")
                    .blank()
                    .text("This action cannot be undone!")
                    .build_string(),
            ),
            DeleteChoice::Cancel => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::ArrowLeft, "Cancel")
                    .blank()
                    .text("Go back without making changes.")
                    .build_string(),
            ),
        }
    }
}

/// Handle deleting a subdirectory
pub(crate) fn handle_delete_subdir(
    repo_name: &str,
    subdir_name: &str,
    config: &mut DotfileConfig,
) -> Result<()> {
    // Get the dotfile repo path
    let dotfile_repo = DotfileRepo::new(config, repo_name.to_string())?;
    let repo_path = dotfile_repo.local_path(config)?;

    // External repos have a fixed structure and cannot have subdirectories removed
    if dotfile_repo.is_external(config) {
        FzfWrapper::message(
            "External repositories use a fixed structure ('.') and cannot have subdirectories added or removed.\n\n\
            To manage subdirectories, convert to a native instantCLI repo by adding an instantdots.toml file.",
        )?;
        return Ok(());
    }

    // Check how many subdirs exist
    let meta = crate::dot::meta::read_meta(&repo_path)?;
    if meta.dots_dirs.len() <= 1 {
        FzfWrapper::message(&format!(
            "Cannot delete '{}' - it's the only dotfile directory in this repository.",
            subdir_name
        ))?;
        return Ok(());
    }

    // Show confirmation with options
    let choices = vec![
        DeleteChoice::KeepFiles,
        DeleteChoice::DeleteFiles,
        DeleteChoice::Cancel,
    ];

    let result = FzfWrapper::builder()
        .header(Header::fancy(&format!("Delete '{}'?", subdir_name)))
        .prompt("How do you want to remove this directory?")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(choices)?;

    match result {
        FzfResult::Selected(DeleteChoice::KeepFiles) => {
            match crate::dot::meta::remove_dots_dir(&repo_path, subdir_name, false) {
                Ok(_) => {
                    remove_from_active_subdirs(config, repo_name, subdir_name)?;
                    FzfWrapper::message(&format!(
                        "Removed '{}' from config. Files kept on disk.",
                        subdir_name
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
        }
        FzfResult::Selected(DeleteChoice::DeleteFiles) => {
            match crate::dot::meta::remove_dots_dir(&repo_path, subdir_name, true) {
                Ok(_) => {
                    remove_from_active_subdirs(config, repo_name, subdir_name)?;
                    FzfWrapper::message(&format!(
                        "Deleted '{}' and all its contents.",
                        subdir_name
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
        }
        FzfResult::Selected(DeleteChoice::Cancel) | FzfResult::Cancelled => {}
        _ => {}
    }

    Ok(())
}

fn remove_from_active_subdirs(
    config: &mut DotfileConfig,
    repo_name: &str,
    subdir_name: &str,
) -> Result<()> {
    if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name)
        && let Some(active_subdirs) = repo.active_subdirectories.as_mut()
    {
        active_subdirs.retain(|s| s != subdir_name);
        config.save(None)?;
    }

    Ok(())
}
