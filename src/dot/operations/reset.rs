use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::utils::{filter_dotfiles_by_path, get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::*;
use std::path::PathBuf;

/// Reset modified dotfiles to their original state
pub fn reset_modified(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    include_root: bool,
    root_only: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db, include_root || root_only)?;
    let target_path = resolve_dotfile_path(path, include_root)?;
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
        if root_only && !dotfile.is_root {
            continue;
        }
        if !root_only && dotfile.is_root {
            continue;
        }

        if !dotfile.is_target_unmodified(db)? {
            dotfile.reset(db)?;
            let relative_path = crate::dot::display_path(&dotfile.target_path, dotfile.is_root);
            println!(
                "{} Reset {} ",
                char::from(NerdFont::Check),
                relative_path.green()
            );
            reset_count += 1;
        } else {
            clean_count += 1;
        }
    }

    if !root_only && include_root {
        let root_files: Vec<_> = filter_dotfiles_by_path(&all_dotfiles, &target_path)
            .into_iter()
            .filter(|d| d.is_root)
            .collect();

        if !root_files.is_empty() {
            let home_dir = std::path::PathBuf::from(shellexpand::tilde("~").to_string());
            let home_dir_str = home_dir.to_string_lossy();
            emit(
                Level::Info,
                "dot.reset.root_files",
                &format!(
                    "{} Resetting {} root dotfile(s) (requires sudo)",
                    char::from(NerdFont::ShieldCheck),
                    root_files.len()
                ),
                None,
            );

            let status = std::process::Command::new("sudo")
                .arg("ins")
                .arg("dot")
                .arg("reset")
                .arg(path)
                .arg("--root-only")
                .arg("--home")
                .arg(home_dir_str.as_ref())
                .status();

            if let Err(e) = status {
                emit(
                    Level::Warn,
                    "dot.reset.root_failed",
                    &format!(
                        "{} Failed to spawn sudo for root dotfiles: {}",
                        char::from(NerdFont::Warning),
                        e
                    ),
                    None,
                );
            } else if let Ok(s) = status {
                if !s.success() {
                    emit(
                        Level::Warn,
                        "dot.reset.root_failed",
                        &format!(
                            "{} Resetting root dotfiles failed or was cancelled",
                            char::from(NerdFont::Warning)
                        ),
                        None,
                    );
                }
            }
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
