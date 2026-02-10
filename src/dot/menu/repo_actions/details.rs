use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::meta;
use crate::dot::repo::DotfileRepositoryManager;
use crate::menu_utils::{
    prompt_text_edit, ConfirmResult, FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor,
    TextEditOutcome, TextEditPrompt,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Detail action for editing repository metadata
#[derive(Debug, Clone)]
enum DetailAction {
    EditAuthor,
    EditDescription,
    ManageUnits,
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
pub(super) fn handle_edit_details(repo_name: &str, config: &DotfileConfig, db: &Database) -> Result<()> {
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
            DetailAction::ManageUnits => {
                handle_manage_units(repo_name, config, db, &repo_path, &mut metadata)?;
            }
            DetailAction::Back => return Ok(()),
        }
    }
}

pub fn handle_global_units_menu(config: &mut DotfileConfig, db: &Database) -> Result<()> {
    let mut cursor = MenuCursor::new();
    let scope = crate::dot::unit_manager::UnitScope::Global;
    let context = crate::dot::unit_manager::unit_path_context_for_write(&scope, config, db)?;

    loop {
        let items = build_global_units_menu_items(config);
        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("Global Units"))
            .prompt("Select unit")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&items) {
            builder = builder.initial_index(index);
        }

        match builder.select_padded(items.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &items);
                match item.action {
                    UnitMenuAction::Add => {
                        if add_global_unit_with_picker(&context, config, db)? {
                            continue;
                        }
                    }
                    UnitMenuAction::Remove(unit) => {
                        let display = crate::dot::unit_manager::unit_display_path(&unit);
                        let confirm = FzfWrapper::builder()
                            .confirm(format!("Remove global unit '{}' ?", display))
                            .yes_text("Remove")
                            .no_text("Cancel")
                            .confirm_dialog()?;
                        if matches!(confirm, ConfirmResult::Yes) {
                            crate::dot::unit_manager::remove_unit(&scope, config, db, &unit, None)?;
                            FzfWrapper::message("Global unit removed")?;
                        }
                    }
                    UnitMenuAction::Back => return Ok(()),
                }
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        }
    }
}

fn load_edit_metadata(
    repo_name: &str,
    config: &DotfileConfig,
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

    let repo_manager = DotfileRepositoryManager::new(config, db);
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
    match prompt_text_edit(TextEditPrompt::new(label, current))? {
        TextEditOutcome::Cancelled | TextEditOutcome::Unchanged => Ok(()),
        TextEditOutcome::Updated(value) => {
            apply_value(metadata, value);
            persist_metadata(repo_path, metadata, success_message)
        }
    }
}

fn handle_manage_units(
    repo_name: &str,
    config: &DotfileConfig,
    db: &Database,
    repo_path: &Path,
    metadata: &mut crate::dot::types::RepoMetaData,
) -> Result<()> {
    let context = crate::dot::unit_manager::unit_path_context_for_write(
        &crate::dot::unit_manager::UnitScope::Repo(repo_name.to_string()),
        config,
        db,
    )?;
    let mut cursor = MenuCursor::new();

    loop {
        let items = build_units_menu_items(metadata, repo_name);
        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("Units: {}", repo_name)))
            .prompt("Select unit")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&items) {
            builder = builder.initial_index(index);
        }

        match builder.select_padded(items.clone())? {
            FzfResult::Selected(item) => {
                cursor.update(&item, &items);
                match item.action {
                    UnitMenuAction::Add => {
                        if add_unit_with_picker(&context, metadata, repo_path, repo_name)? {
                            continue;
                        }
                    }
                    UnitMenuAction::Remove(unit) => {
                        let display = crate::dot::unit_manager::unit_display_path(&unit);
                        let confirm = FzfWrapper::builder()
                            .confirm(format!("Remove unit '{}' from {}?", display, repo_name))
                            .yes_text("Remove")
                            .no_text("Cancel")
                            .confirm_dialog()?;
                        if matches!(confirm, ConfirmResult::Yes) {
                            metadata.units.retain(|entry| entry != &unit);
                            persist_metadata(
                                repo_path,
                                metadata,
                                "Unit removed from repository metadata",
                            )?;
                        }
                    }
                    UnitMenuAction::Back => return Ok(()),
                }
            }
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        }
    }
}

#[derive(Clone)]
struct UnitMenuItem {
    display: String,
    preview: String,
    action: UnitMenuAction,
}

#[derive(Clone)]
enum UnitMenuAction {
    Add,
    Remove(String),
    Back,
}

impl FzfSelectable for UnitMenuItem {
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

fn build_units_menu_items(
    metadata: &crate::dot::types::RepoMetaData,
    repo_name: &str,
) -> Vec<UnitMenuItem> {
    let mut items = Vec::new();

    items.push(UnitMenuItem {
        display: format!(
            "{} Add Unit",
            format_icon_colored(NerdFont::Plus, colors::GREEN)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::Folder, "Add Unit")
            .text("Units group related dotfiles so they update together.")
            .blank()
            .text("Choose a directory from this repo or your home directory.")
            .blank()
            .subtext("Best for repo authors: declare atomic config folders.")
            .build_string(),
        action: UnitMenuAction::Add,
    });

    for unit in &metadata.units {
        let display = crate::dot::unit_manager::unit_display_path(unit);
        items.push(UnitMenuItem {
            display: format!(
                "{} {}",
                format_icon_colored(NerdFont::Trash, colors::RED),
                display
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Trash, "Remove Unit")
                .field("Repository", repo_name)
                .field("Unit", &display)
                .blank()
                .subtext("Removing the unit keeps files tracked but stops atomic updates.")
                .build_string(),
            action: UnitMenuAction::Remove(unit.clone()),
        });
    }

    items.push(UnitMenuItem {
        display: format!("{} Back", format_back_icon()),
        preview: PreviewBuilder::new()
            .subtext("Return to edit details")
            .build_string(),
        action: UnitMenuAction::Back,
    });

    items
}

fn build_global_units_menu_items(config: &DotfileConfig) -> Vec<UnitMenuItem> {
    let mut items = Vec::new();
    items.push(UnitMenuItem {
        display: format!(
            "{} Add Global Unit",
            format_icon_colored(NerdFont::Plus, colors::GREEN)
        ),
        preview: PreviewBuilder::new()
            .header(NerdFont::FolderConfig, "Add Global Unit")
            .text("Global units apply to every repository.")
            .blank()
            .text("Use these sparingly; repo-scoped units are preferred.")
            .blank()
            .subtext("Ideal for personal overrides outside any repo.")
            .build_string(),
        action: UnitMenuAction::Add,
    });

    for unit in &config.units {
        let display = crate::dot::unit_manager::unit_display_path(unit);
        items.push(UnitMenuItem {
            display: format!(
                "{} {}",
                format_icon_colored(NerdFont::Trash, colors::RED),
                display
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Trash, "Remove Global Unit")
                .field("Unit", &display)
                .blank()
                .subtext("Removing a global unit stops atomic updates for that path.")
                .build_string(),
            action: UnitMenuAction::Remove(unit.clone()),
        });
    }

    items.push(UnitMenuItem {
        display: format!("{} Back", format_back_icon()),
        preview: PreviewBuilder::new()
            .subtext("Return to dot menu")
            .build_string(),
        action: UnitMenuAction::Back,
    });

    items
}

fn add_global_unit_with_picker(
    context: &crate::dot::unit_manager::UnitPathContext,
    config: &mut DotfileConfig,
    db: &Database,
) -> Result<bool> {
    use crate::menu_utils::{FilePickerScope, MenuWrapper};

    let picked = match MenuWrapper::file_picker()
        .start_dir(&context.home)
        .scope(FilePickerScope::Directories)
        .show_hidden(true)
        .hint("Pick a directory in your home folder")
        .pick_one()
    {
        Ok(Some(path)) => path,
        Ok(None) => return Ok(false),
        Err(e) => {
            FzfWrapper::message(&format!("File picker error: {}", e))?;
            return Ok(false);
        }
    };

    let normalized = match crate::dot::unit_manager::normalize_unit_fs_path(&picked, context) {
        Ok(path) => path,
        Err(e) => {
            FzfWrapper::message(&format!("Invalid unit path: {}", e))?;
            return Ok(false);
        }
    };

    if config.units.contains(&normalized) {
        FzfWrapper::message("That unit is already configured globally")?;
        return Ok(false);
    }

    crate::dot::unit_manager::add_unit(
        &crate::dot::unit_manager::UnitScope::Global,
        config,
        db,
        &normalized,
        None,
    )?;
    FzfWrapper::message("Global unit added")?;
    Ok(true)
}

fn add_unit_with_picker(
    context: &crate::dot::unit_manager::UnitPathContext,
    metadata: &mut crate::dot::types::RepoMetaData,
    repo_path: &Path,
    repo_name: &str,
) -> Result<bool> {
    use crate::menu_utils::{FilePickerScope, MenuWrapper};

    let start_dir = context
        .repo
        .as_ref()
        .map(|repo| repo.path.clone())
        .unwrap_or_else(|| context.home.clone());

    let hint = format!("Pick a directory in {} or your home folder", repo_name);

    let picked = match MenuWrapper::file_picker()
        .start_dir(start_dir)
        .scope(FilePickerScope::Directories)
        .show_hidden(true)
        .hint(hint)
        .pick_one()
    {
        Ok(Some(path)) => path,
        Ok(None) => return Ok(false),
        Err(e) => {
            FzfWrapper::message(&format!("File picker error: {}", e))?;
            return Ok(false);
        }
    };

    let normalized = match crate::dot::unit_manager::normalize_unit_fs_path(&picked, context) {
        Ok(path) => path,
        Err(e) => {
            FzfWrapper::message(&format!("Invalid unit path: {}", e))?;
            return Ok(false);
        }
    };

    if metadata.units.contains(&normalized) {
        FzfWrapper::message("That unit is already configured in this repo")?;
        return Ok(false);
    }

    metadata.units.push(normalized.clone());
    persist_metadata(repo_path, metadata, "Unit added to repository metadata")?;
    Ok(true)
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

    let units_count = metadata.units.len();
    let units_label = if units_count == 0 {
        "(none)".to_string()
    } else {
        format!("{} configured", units_count)
    };
    actions.push(DetailActionItem {
        display: format!(
            "{} Units",
            format_icon_colored(NerdFont::FolderConfig, colors::TEAL)
        ),
        preview: PreviewBuilder::new()
            .line(colors::TEAL, Some(NerdFont::FolderConfig), "Manage Units")
            .blank()
            .field("Current", &units_label)
            .blank()
            .text("Units are directories treated as atomic updates.")
            .text("If any file in a unit is modified, all files in the unit are protected.")
            .blank()
            .subtext("This is primarily for repo authors defining safe update boundaries.")
            .build_string(),
        action: DetailAction::ManageUnits,
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
