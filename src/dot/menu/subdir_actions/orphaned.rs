//! Orphaned subdirectory resolution actions.

use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::localrepo::LocalRepo;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Orphaned subdir action
#[derive(Clone)]
enum OrphanedAction {
    Disable,
    AddToMetadata,
    Back,
}

impl FzfSelectable for OrphanedAction {
    fn fzf_display_text(&self) -> String {
        match self {
            OrphanedAction::Disable => format!(
                "{} Disable",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            OrphanedAction::AddToMetadata => format!(
                "{} Add to Metadata",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            OrphanedAction::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            OrphanedAction::Disable => "disable".to_string(),
            OrphanedAction::AddToMetadata => "add".to_string(),
            OrphanedAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        use crate::menu::protocol::FzfPreview;
        match self {
            OrphanedAction::Disable => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Trash, "Disable Subdirectory")
                    .blank()
                    .text("Remove this subdirectory from the enabled list.")
                    .blank()
                    .text("This fixes the mismatch by removing the entry")
                    .text("from your active_subdirectories config.")
                    .build_string(),
            ),
            OrphanedAction::AddToMetadata => FzfPreview::Text(
                PreviewBuilder::new()
                    .header(NerdFont::Plus, "Add to Metadata")
                    .blank()
                    .text("Add this subdirectory to the repository's")
                    .text("instantdots.toml metadata file.")
                    .blank()
                    .text("This fixes the mismatch by declaring the")
                    .text("subdirectory in the repository.")
                    .build_string(),
            ),
            OrphanedAction::Back => FzfPreview::Text("Return to subdirectory list".to_string()),
        }
    }
}

/// Handle orphaned subdir resolution actions
pub(crate) fn handle_orphaned_subdir_actions(
    repo_name: &str,
    subdir_name: &str,
    local_repo: &LocalRepo,
    config: &mut Config,
) -> Result<()> {
    let actions = vec![
        OrphanedAction::Disable,
        OrphanedAction::AddToMetadata,
        OrphanedAction::Back,
    ];

    let mut cursor = MenuCursor::new();

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy(&format!("Fix: {} [mismatch]", subdir_name)))
        .prompt("Select action")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&actions) {
        builder = builder.initial_index(index);
    }

    let result = builder.select(actions.clone())?;

    let action = match result {
        FzfResult::Selected(item) => {
            cursor.update(&item, &actions);
            item
        }
        FzfResult::Cancelled => return Ok(()),
        _ => return Ok(()),
    };

    match action {
        OrphanedAction::Disable => {
            let mut should_save = false;
            if let Some(repo) = config.repos.iter_mut().find(|r| r.name == repo_name)
                && let Some(active_subdirs) = repo.active_subdirectories.as_mut()
            {
                active_subdirs.retain(|s| s != subdir_name);
                should_save = true;
            }
            if should_save {
                config.save(None)?;
                FzfWrapper::message(&format!("Disabled '{}'. Mismatch resolved.", subdir_name))?;
            }
        }
        OrphanedAction::AddToMetadata => {
            let repo_path = local_repo.local_path(config)?;
            match crate::dot::meta::add_dots_dir(&repo_path, subdir_name) {
                Ok(()) => {
                    FzfWrapper::message(&format!(
                        "Added '{}' to instantdots.toml. Mismatch resolved.",
                        subdir_name
                    ))?;
                }
                Err(e) => {
                    FzfWrapper::message(&format!("Error: {}", e))?;
                }
            }
        }
        OrphanedAction::Back => {}
    }

    Ok(())
}
