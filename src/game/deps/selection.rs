use anyhow::{Context, Result};

use std::path::PathBuf;

use crate::game::config::GameDependency;
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

#[derive(Clone)]
pub struct DependencyOption<'a> {
    game_name: &'a str,
    dependency: &'a GameDependency,
}

impl<'a> DependencyOption<'a> {
    pub fn new(game_name: &'a str, dependency: &'a GameDependency) -> Self {
        Self {
            game_name,
            dependency,
        }
    }
}

impl<'a> FzfSelectable for DependencyOption<'a> {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", char::from(NerdFont::Package), self.dependency.id)
    }

    fn fzf_key(&self) -> String {
        self.dependency.id.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut preview = String::new();
        preview.push_str(&format!(
            "{} DEPENDENCY DETAILS\n\n",
            char::from(NerdFont::Package)
        ));
        preview.push_str(&format!("Game: {}\n", self.game_name));
        preview.push_str(&format!("ID: {}\n", self.dependency.id));
        preview.push_str(&format!(
            "Source path: {}\n",
            format_path_for_display(&self.dependency.source_path)
        ));
        preview.push_str(&format!("Kind: {:?}\n", self.dependency.kind));

        if let Some(snapshot) = &self.dependency.snapshot_id {
            preview.push_str(&format!("Snapshot: {}\n", snapshot));
        } else {
            preview.push_str("Snapshot: <not captured yet>\n");
        }

        FzfPreview::Text(preview)
    }
}

pub fn select_dependency<'a>(
    game_name: &'a str,
    dependencies: &'a [GameDependency],
) -> Result<Option<&'a GameDependency>> {
    if dependencies.is_empty() {
        println!(
            "{} Game '{}' has no registered dependencies.",
            char::from(NerdFont::Info),
            game_name
        );
        return Ok(None);
    }

    let options: Vec<DependencyOption<'_>> = dependencies
        .iter()
        .map(|dependency| DependencyOption::new(game_name, dependency))
        .collect();

    let selection =
        FzfWrapper::select_one(options).context("Failed to select dependency interactively")?;

    Ok(selection.map(|option| option.dependency))
}

fn format_path_for_display(path: &str) -> String {
    let path_buf = PathBuf::from(path);
    crate::dot::path_serde::TildePath::new(path_buf)
        .to_tilde_string()
        .unwrap_or_else(|_| path.to_string())
}
