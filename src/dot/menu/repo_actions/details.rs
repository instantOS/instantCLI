use anyhow::Result;

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::meta;
use crate::dot::repo::RepositoryManager;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Detail action for editing repository metadata
#[derive(Debug, Clone)]
enum DetailAction {
    EditAuthor,
    EditDescription,
    Back,
}

#[derive(Clone)]
struct DetailActionItem {
    display: String,
    preview: String,
    action: DetailAction,
}

impl FzfSelectable for DetailActionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.display.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

/// Handle editing repository details
pub(super) fn handle_edit_details(repo_name: &str, config: &Config, db: &Database) -> Result<()> {
    let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
        Some(rc) => rc,
        None => {
            FzfWrapper::message(&format!("Repository '{}' not found in config", repo_name))?;
            return Ok(());
        }
    };

    // Check if read-only
    if repo_config.read_only {
        FzfWrapper::message(&format!(
            "Repository '{}' is read-only. Cannot edit metadata.",
            repo_name
        ))?;
        return Ok(());
    }

    // External repos have metadata in global config - not supported for now
    if repo_config.metadata.is_some() {
        FzfWrapper::message(
            "External repositories have metadata in global config.\n\
            Editing external repo metadata is not currently supported.",
        )?;
        return Ok(());
    }

    let repo_manager = RepositoryManager::new(config, db);
    let local_repo = match repo_manager.get_repository_info(repo_name) {
        Ok(lr) => lr,
        Err(e) => {
            FzfWrapper::message(&format!("Failed to load repository: {}", e))?;
            return Ok(());
        }
    };

    let repo_path = match local_repo.local_path(config) {
        Ok(p) => p,
        Err(e) => {
            FzfWrapper::message(&format!("Failed to get repository path: {}", e))?;
            return Ok(());
        }
    };

    // Read current metadata
    let mut metadata = match meta::read_meta(&repo_path) {
        Ok(m) => m,
        Err(e) => {
            FzfWrapper::message(&format!("Failed to read metadata: {}", e))?;
            return Ok(());
        }
    };

    let mut cursor = MenuCursor::new();

    loop {
        let actions = build_detail_action_menu(&metadata, repo_name);

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("Edit Details: {}", repo_name)))
            .prompt("Select field to edit")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&actions) {
            builder = builder.initial_index(index);
        }

        let result = builder.select_padded(actions.clone())?;

        let action = match result {
            FzfResult::Selected(item) => {
                cursor.update(&item, &actions);
                item.action
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        };

        match action {
            DetailAction::EditAuthor => {
                let current = metadata.author.as_deref().unwrap_or("");
                let ghost_text = if current.is_empty() {
                    "(no author set)"
                } else {
                    current
                };

                let new_value = match FzfWrapper::builder()
                    .input()
                    .prompt("Author")
                    .ghost(ghost_text)
                    .input_result()?
                {
                    FzfResult::Selected(s) => Some(s.trim().to_string()),
                    FzfResult::Cancelled => continue,
                    _ => continue,
                };

                // Empty string means clear the field
                let new_value = if new_value.as_ref().map(|s| s.is_empty()).unwrap_or(false) {
                    None
                } else {
                    new_value
                };

                metadata.author = new_value;

                match meta::update_meta(&repo_path, &metadata) {
                    Ok(_) => {
                        FzfWrapper::message("Author updated successfully")?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Failed to update metadata: {}", e))?;
                    }
                }
            }
            DetailAction::EditDescription => {
                let current = metadata.description.as_deref().unwrap_or("");
                let ghost_text = if current.is_empty() {
                    "(no description set)"
                } else {
                    current
                };

                let new_value = match FzfWrapper::builder()
                    .input()
                    .prompt("Description")
                    .ghost(ghost_text)
                    .input_result()?
                {
                    FzfResult::Selected(s) => Some(s.trim().to_string()),
                    FzfResult::Cancelled => continue,
                    _ => continue,
                };

                // Empty string means clear the field
                let new_value = if new_value.as_ref().map(|s| s.is_empty()).unwrap_or(false) {
                    None
                } else {
                    new_value
                };

                metadata.description = new_value;

                match meta::update_meta(&repo_path, &metadata) {
                    Ok(_) => {
                        FzfWrapper::message("Description updated successfully")?;
                    }
                    Err(e) => {
                        FzfWrapper::message(&format!("Failed to update metadata: {}", e))?;
                    }
                }
            }
            DetailAction::Back => return Ok(()),
        }
    }
}

/// Build the detail action menu items
fn build_detail_action_menu(
    metadata: &crate::dot::types::RepoMetaData,
    _repo_name: &str,
) -> Vec<DetailActionItem> {
    let mut actions = Vec::new();

    // Edit Author
    let author_value = metadata.author.as_deref().unwrap_or("(none)");
    actions.push(DetailActionItem {
        display: format!(
            "{} Author",
            format_icon_colored(NerdFont::User, colors::BLUE)
        ),
        preview: PreviewBuilder::new()
            .line(colors::BLUE, Some(NerdFont::User), "Edit Author")
            .blank()
            .field("Current", author_value)
            .blank()
            .subtext("Set or change the repository author/maintainer")
            .build_string(),
        action: DetailAction::EditAuthor,
    });

    // Edit Description
    let desc_value = metadata.description.as_deref().unwrap_or("(none)");
    actions.push(DetailActionItem {
        display: format!(
            "{} Description",
            format_icon_colored(NerdFont::FileText, colors::MAUVE)
        ),
        preview: PreviewBuilder::new()
            .line(colors::MAUVE, Some(NerdFont::FileText), "Edit Description")
            .blank()
            .field("Current", desc_value)
            .blank()
            .subtext("Set or change the repository description")
            .build_string(),
        action: DetailAction::EditDescription,
    });

    // Back
    actions.push(DetailActionItem {
        display: format!("{} Back", format_back_icon()),
        preview: PreviewBuilder::new()
            .subtext("Return to repository menu")
            .build_string(),
        action: DetailAction::Back,
    });

    actions
}
