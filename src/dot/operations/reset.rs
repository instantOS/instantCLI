use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::utils::{filter_dotfiles_by_path, get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::*;
use std::path::PathBuf;

/// Reset modified dotfiles to their original state
pub fn reset_modified(config: &DotfileConfig, db: &Database, path: &str) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db)?;
    let target_path = resolve_dotfile_path(path)?;
    let home = PathBuf::from(shellexpand::tilde("~").to_string());

    // Filter to dotfiles within the specified path
    let dotfiles_in_path = filter_dotfiles_by_path(&all_dotfiles, &target_path);

    if dotfiles_in_path.is_empty() {
        let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
        emit(
            Level::Info,
            "dot.reset.no_files",
            &format!(
                "{} No tracked dotfiles found in ~/{} ",
                char::from(NerdFont::Info),
                relative_path.display()
            ),
            None,
        );
        return Ok(());
    }

    let mut reset_count = 0;
    let mut clean_count = 0;

    for dotfile in dotfiles_in_path {
        if !dotfile.is_target_unmodified(db)? {
            dotfile.reset(db)?;
            let relative_path = dotfile
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&dotfile.target_path);
            println!(
                "{} Reset ~/{} ",
                char::from(NerdFont::Check),
                relative_path.display().to_string().green()
            );
            reset_count += 1;
        } else {
            clean_count += 1;
        }
    }

    db.cleanup_hashes(config.hash_cleanup_days)?;

    // Summary
    if reset_count > 0 {
        let reset_text = if reset_count == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", reset_count)
        };

        let msg = if clean_count > 0 {
            let clean_text = if clean_count == 1 {
                "1 already clean".to_string()
            } else {
                format!("{} already clean", clean_count)
            };
            format!(
                "{} Reset {}, {}",
                char::from(NerdFont::Check),
                reset_text,
                clean_text
            )
        } else {
            format!("{} Reset {}", char::from(NerdFont::Check), reset_text)
        };

        emit(Level::Success, "dot.reset.complete", &msg, None);
    } else {
        let clean_text = if clean_count == 1 {
            "1 file is already clean".to_string()
        } else {
            format!("All {} files already clean", clean_count)
        };

        emit(
            Level::Info,
            "dot.reset.no_changes",
            &format!(
                "{} {} - no reset needed",
                char::from(NerdFont::Info),
                clean_text
            ),
            None,
        );
    }

    Ok(())
}
