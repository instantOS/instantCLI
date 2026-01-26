//! Default enabled subdirectory selection for a repo.

use anyhow::Result;
use std::collections::HashSet;

use crate::dot::config::Config;
use crate::dot::localrepo::LocalRepo;
use crate::dot::meta;
use crate::dot::types::RepoMetaData;
use crate::menu_utils::{ChecklistAction, ChecklistResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

const AUTO_DEFAULTS_KEY: &str = "__auto_defaults__";

#[derive(Clone)]
struct DefaultSubdirItem {
    name: String,
    checked: bool,
}

impl FzfSelectable for DefaultSubdirItem {
    fn fzf_display_text(&self) -> String {
        self.name.clone()
    }

    fn fzf_key(&self) -> String {
        self.name.clone()
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(
            PreviewBuilder::new()
                .header(NerdFont::Folder, &self.name)
                .text("Include this subdirectory in the default enabled set.")
                .build_string(),
        )
    }

    fn fzf_initial_checked_state(&self) -> bool {
        self.checked
    }
}

pub(crate) fn handle_edit_default_subdirs(
    repo_name: &str,
    local_repo: &LocalRepo,
    config: &Config,
) -> Result<()> {
    if local_repo.is_external(config) {
        FzfWrapper::message(
            "External repositories use metadata from the global config and cannot be edited here.",
        )?;
        return Ok(());
    }

    let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
        Some(repo) => repo,
        None => {
            FzfWrapper::message(&format!("Repository '{}' not found", repo_name))?;
            return Ok(());
        }
    };

    if repo_config.read_only {
        FzfWrapper::message(&format!(
            "Repository '{}' is read-only. Cannot edit default subdirectories.",
            repo_name
        ))?;
        return Ok(());
    }

    if local_repo.meta.dots_dirs.is_empty() {
        FzfWrapper::message("No dotfile directories are defined for this repository.")?;
        return Ok(());
    }

    let repo_path = local_repo.local_path(config)?;
    let mut metadata = local_repo.meta.clone();
    let current_set: HashSet<String> = metadata
        .default_active_subdirs
        .as_ref()
        .map(|defaults| {
            defaults
                .iter()
                .filter(|dir| metadata.dots_dirs.contains(*dir))
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let items: Vec<DefaultSubdirItem> = metadata
        .dots_dirs
        .iter()
        .map(|dir| DefaultSubdirItem {
            name: dir.clone(),
            checked: current_set.contains(dir),
        })
        .collect();

    let auto_action = ChecklistAction::new(AUTO_DEFAULTS_KEY, format_auto_option_label(&metadata))
        .with_color(colors::YELLOW)
        .with_preview(crate::menu::protocol::FzfPreview::Text(
            PreviewBuilder::new()
                .header(NerdFont::Star, "Auto defaults")
                .text("Remove explicit defaults so the first subdir is enabled by default.")
                .text("Use this when you want defaults to follow the repo's first subdir.")
                .build_string(),
        ));

    let selection = FzfWrapper::builder()
        .checklist("Save Defaults")
        .prompt("Toggle defaults")
        .header(Header::fancy(&format!(
            "Default enabled: {}\nUse Auto to reset | Select none to disable defaults",
            repo_name
        )))
        .checklist_actions(vec![auto_action])
        .args(fzf_mocha_args())
        .responsive_layout()
        .checklist_dialog(items)?;

    let new_defaults = match selection {
        ChecklistResult::Confirmed(items) => {
            let selected_names: Vec<String> = items.into_iter().map(|item| item.name).collect();
            Some(normalize_default_active_subdirs(&metadata, selected_names))
        }
        ChecklistResult::Action(action) => {
            if action.key == AUTO_DEFAULTS_KEY {
                None
            } else {
                return Ok(());
            }
        }
        ChecklistResult::Cancelled => return Ok(()),
    };

    if metadata.default_active_subdirs == new_defaults {
        FzfWrapper::message("Default enabled subdirectories unchanged.")?;
        return Ok(());
    }

    metadata.default_active_subdirs = new_defaults.clone();

    match meta::update_meta(&repo_path, &metadata) {
        Ok(()) => match new_defaults {
            Some(defaults) => {
                if defaults.is_empty() {
                    FzfWrapper::message(
                        "Default enabled subdirectories cleared. Repo is disabled until you enable subdirs.",
                    )?;
                } else {
                    FzfWrapper::message(&format!(
                        "Default enabled subdirectories updated: {}",
                        defaults.join(", ")
                    ))?;
                }
            }
            None => {
                let fallback = metadata
                    .dots_dirs
                    .first()
                    .map(|dir| dir.as_str())
                    .unwrap_or("(none)");
                FzfWrapper::message(&format!(
                    "Default enabled subdirectories reset to auto (first: {}).",
                    fallback
                ))?;
            }
        },
        Err(e) => {
            FzfWrapper::message(&format!("Error: {}", e))?;
        }
    }

    Ok(())
}

fn resolve_default_active_subdirs(meta: &RepoMetaData) -> Vec<String> {
    if let Some(defaults) = meta.default_active_subdirs.as_ref() {
        return defaults
            .iter()
            .filter(|dir| meta.dots_dirs.contains(*dir))
            .cloned()
            .collect();
    }

    meta.dots_dirs.first().cloned().into_iter().collect()
}

fn normalize_default_active_subdirs(meta: &RepoMetaData, selected: Vec<String>) -> Vec<String> {
    let selected_set: HashSet<String> = selected.into_iter().collect();
    let normalized: Vec<String> = meta
        .dots_dirs
        .iter()
        .filter(|dir| selected_set.contains(*dir))
        .cloned()
        .collect();

    normalized
}

fn format_auto_option_label(meta: &RepoMetaData) -> String {
    let default = meta
        .dots_dirs
        .first()
        .map(|dir| dir.as_str())
        .unwrap_or("none");
    format!("Auto (first: {default})")
}

pub(crate) fn format_default_active_label(meta: &RepoMetaData) -> String {
    let defaults = resolve_default_active_subdirs(meta);

    match meta.default_active_subdirs.as_ref() {
        None => {
            let default = defaults.first().map(|s| s.as_str()).unwrap_or("none");
            format!("Auto (first: {})", default)
        }
        Some(dirs) if dirs.is_empty() || defaults.is_empty() => "Disabled (none)".to_string(),
        Some(_) => defaults.join(", "),
    }
}
