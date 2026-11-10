mod json;
mod report;
mod single;
#[cfg(test)]
mod tests;
mod text;

pub use report::{DotFileStatus, collect_status, get_dotfile_status};
pub use single::show_single_file_status;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dot::config::DotfileConfig;
use crate::dot::units::UnitIndex;
use crate::ui::prelude::*;

pub fn show_status_summary(
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &DotfileConfig,
    db: &crate::dot::db::Database,
    show_all: bool,
    show_sources: bool,
    unit_index: &UnitIndex,
) -> Result<()> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let context = RenderContext {
        home: &home,
        config: cfg,
    };
    // File status is independent of unit protection. Unchanged members of a modified
    // unit remain clean here, while the renderers report modified units separately.
    let report = collect_status(all_dotfiles, cfg, db, &UnitIndex::default());
    match get_output_format() {
        OutputFormat::Json => emit(
            Level::Info,
            "dot.status.summary",
            "Dotfile status summary",
            Some(json::document(
                &report,
                show_all,
                show_sources,
                &context,
                unit_index,
            )),
        ),
        OutputFormat::Text => text::render(&report, show_all, show_sources, &context, unit_index),
    }
    Ok(())
}

struct RenderContext<'a> {
    home: &'a Path,
    config: &'a DotfileConfig,
}

impl RenderContext<'_> {
    fn path(&self, path: &Path) -> String {
        match path.strip_prefix(self.home) {
            Ok(relative) => format!("~/{}", relative.display()),
            Err(_) => path.display().to_string(),
        }
    }

    fn priority(&self, repo_name: &str) -> usize {
        self.config
            .repos
            .iter()
            .position(|repo| repo.name == repo_name)
            .map_or(0, |index| index + 1)
    }
}
