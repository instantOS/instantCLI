use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::meta;
use crate::dot::repo::RepositoryManager;
use crate::menu_utils::{
    FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor, TextEditOutcome, prompt_text_edit,
};
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
    let Some((repo_path, mut metadata)) = load_edit_metadata(repo_name, config, db)? else {
        return Ok(());
    };

    let mut cursor = MenuCursor::new();

    loop {
        let Some(action) = select_detail_action(repo_name, &metadata, &mut cursor)? else {
            return Ok(());
        };

        match action {
            DetailAction::EditAuthor => {
                let current_author = metadata.author.clone();
                apply_text_edit(
                    "Author",
                    current_author.as_deref(),
                    &repo_path,
                    &mut metadata,
                    "Author updated successfully",
                    |metadata, value| metadata.author = value,
                )?;
            }
            DetailAction::EditDescription => {
                let current_description = metadata.description.clone();
                apply_text_edit(
                    "Description",
                    current_description.as_deref(),
                    &repo_path,
                    &mut metadata,
                    "Description updated successfully",
                    |metadata, value| metadata.description = value,
                )?;
            }
            DetailAction::Back => return Ok(()),
        }
    }
}

fn load_edit_metadata(
    repo_name: &str,
    config: &Config,
    db: &Database,
) -> Result<Option<(PathBuf, crate::dot::types::RepoMetaData)>> {
    let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
        Some(rc) => rc,
        None => {
            FzfWrapper::message(&format!("Repository '{}' not found in config", repo_name))?;
            return Ok(None);
        }
    };

    if repo_config.read_only {
        FzfWrapper::message(&format!(
            "Repository '{}' is read-only. Cannot edit metadata.",
            repo_name
        ))?;
        return Ok(None);
    }

    if repo_config.metadata.is_some() {
        FzfWrapper::message(
            "External repositories have metadata in global config.\n\
            Editing external repo metadata is not currently supported.",
        )?;
        return Ok(None);
    }

    let repo_manager = RepositoryManager::new(config, db);
    let local_repo = match repo_manager.get_repository_info(repo_name) {
        Ok(lr) => lr,
        Err(e) => {
            FzfWrapper::message(&format!("Failed to load repository: {}", e))?;
            return Ok(None);
        }
    };

    let repo_path = match local_repo.local_path(config) {
        Ok(p) => p,
        Err(e) => {
            FzfWrapper::message(&format!("Failed to get repository path: {}", e))?;
            return Ok(None);
        }
    };

    let metadata = match meta::read_meta(&repo_path) {
        Ok(m) => m,
        Err(e) => {
            FzfWrapper::message(&format!("Failed to read metadata: {}", e))?;
            return Ok(None);
        }
    };

    Ok(Some((repo_path, metadata)))
}

fn select_detail_action(
    repo_name: &str,
    metadata: &crate::dot::types::RepoMetaData,
    cursor: &mut MenuCursor,
) -> Result<Option<DetailAction>> {
    let actions = build_detail_action_menu(metadata, repo_name);

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy(&format!("Edit Details: {}", repo_name)))
        .prompt("Select field to edit")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&actions) {
        builder = builder.initial_index(index);
    }

    let result = builder.select_padded(actions.clone())?;

    match result {
        FzfResult::Selected(item) => {
            cursor.update(&item, &actions);
            Ok(Some(item.action))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

fn apply_text_edit<F>(
    label: &str,
    current: Option<&str>,
    repo_path: &Path,
    metadata: &mut crate::dot::types::RepoMetaData,
    success_message: &str,
    apply_value: F,
) -> Result<()>
where
    F: FnOnce(&mut crate::dot::types::RepoMetaData, Option<String>),
{
    match prompt_text_edit(label, current)? {
        TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => Ok(()),
        TextEditOutcome::Updated(value) => {
            apply_value(metadata, value);
            persist_metadata(repo_path, metadata, success_message)
        }
    }
}

fn persist_metadata(
    repo_path: &Path,
    metadata: &crate::dot::types::RepoMetaData,
    success_message: &str,
) -> Result<()> {
    match meta::update_meta(repo_path, metadata) {
        Ok(_) => {
            FzfWrapper::message(success_message)?;
        }
        Err(e) => {
            FzfWrapper::message(&format!("Failed to update metadata: {}", e))?;
        }
    }
    Ok(())
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
