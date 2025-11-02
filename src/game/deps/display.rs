use anyhow::Result;
use serde_json::json;

use crate::game::config::{GameDependency, GameInstallation};
use crate::ui::nerd_font::NerdFont;
use crate::ui::prelude::*;

pub fn show_dependency_list(
    game_name: &str,
    dependencies: &[GameDependency],
    installation: Option<&GameInstallation>,
) -> Result<()> {
    if dependencies.is_empty() {
        emit(
            Level::Info,
            "game.deps.list.empty",
            &format!(
                "{} Game '{}' has no registered dependencies.",
                char::from(NerdFont::Info),
                game_name
            ),
            Some(json!({
                "game": game_name,
                "dependency_count": 0
            })),
        );
        return Ok(());
    }

    let mut text = String::new();
    text.push_str(&format!(
        "{} Dependencies for '{}'\n\n",
        char::from(NerdFont::Package),
        game_name
    ));

    let mut data = Vec::new();
    for dependency in dependencies {
        let installed_path = installation
            .and_then(|inst| {
                inst.dependencies
                    .iter()
                    .find(|installed| installed.dependency_id == dependency.id)
            })
            .and_then(|installed| installed.install_path.to_tilde_string().ok());

        let kind_label = if dependency.source_type.is_file() {
            "File"
        } else {
            "Directory"
        };
        text.push_str(&format!("   • Kind: {kind_label}\n"));
        if let Some(path) = &installed_path {
            text.push_str(&format!("   • Installed at: {}\n", path));
        } else {
            text.push_str("   • Installed at: <not installed>\n");
        }
        text.push('\n');

        data.push(json!({
            "id": dependency.id,
            "source_path": dependency.source_path,
            "kind": kind_label,
            "installed_path": installed_path,
        }));
    }

    emit(
        Level::Info,
        "game.deps.list",
        &text,
        Some(json!({
            "game": game_name,
            "dependency_count": dependencies.len(),
            "dependencies": data
        })),
    );

    Ok(())
}

fn format_path_for_display(path: &str) -> String {
    let path_buf = std::path::PathBuf::from(path);
    crate::dot::path_serde::TildePath::new(path_buf)
        .to_tilde_string()
        .unwrap_or_else(|_| path.to_string())
}
