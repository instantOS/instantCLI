use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::utils::{
    EmptyParentBoundary, clean_empty_parent_dirs, filter_dotfiles_by_path, get_all_dotfiles,
    resolve_dotfile_path,
};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::prelude::*;
use anyhow::Result;
use colored::*;
use std::collections::HashSet;

type DotfileMap = std::collections::HashMap<std::path::PathBuf, crate::dot::Dotfile>;

/// A dotfile entry for FZF selection
#[derive(Clone)]
struct DotfileEntry {
    display: String,
    target_path: std::path::PathBuf,
}

impl FzfSelectable for DotfileEntry {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }
}

pub fn delete_dotfiles(
    config: &DotfileConfig,
    db: &Database,
    path: Option<&str>,
    recursive: bool,
    dry_run: bool,
    debug: bool,
) -> Result<()> {
    let all_dotfiles = get_all_dotfiles(config, db, false)?;

    let dotfiles_to_delete = if let Some(path) = path {
        resolve_path_dotfiles(path, recursive, &all_dotfiles)?
    } else {
        pick_dotfiles_fzf(&all_dotfiles)?
    };

    if dotfiles_to_delete.is_empty() {
        emit(
            Level::Info,
            "dot.delete.no_files",
            &format!("{} No dotfiles to delete", char::from(NerdFont::Info)),
            None,
        );
        return Ok(());
    }

    if dry_run {
        for dotfile in &dotfiles_to_delete {
            let display = crate::dot::display_path(&dotfile.target_path, dotfile.is_root);
            println!(
                "{} Would delete {} (repo: {})",
                char::from(NerdFont::Info).to_string().yellow(),
                display.cyan(),
                dotfile.source_path.display()
            );
        }
        emit(
            Level::Info,
            "dot.delete.dry_run",
            &format!(
                "{} Dry run: {} dotfile(s) would be deleted",
                char::from(NerdFont::Info),
                dotfiles_to_delete.len()
            ),
            None,
        );
        return Ok(());
    }

    let mut deleted_count = 0;
    for dotfile in &dotfiles_to_delete {
        delete_single_dotfile(db, dotfile, debug)?;
        deleted_count += 1;
    }

    let msg = if deleted_count == 1 {
        "1 dotfile".to_string()
    } else {
        format!("{} dotfiles", deleted_count)
    };
    emit(
        Level::Success,
        "dot.delete.complete",
        &format!("{} Deleted {}", char::from(NerdFont::Check), msg),
        None,
    );

    Ok(())
}

fn resolve_path_dotfiles(
    path: &str,
    recursive: bool,
    all_dotfiles: &DotfileMap,
) -> Result<Vec<crate::dot::Dotfile>> {
    let target_path = resolve_dotfile_path(path, false, false)?;
    let dotfiles_in_path = filter_dotfiles_by_path(all_dotfiles, &target_path);

    if !recursive {
        let exact: Vec<_> = dotfiles_in_path
            .into_iter()
            .filter(|d| d.target_path == target_path)
            .cloned()
            .collect();

        if exact.is_empty() {
            let display = crate::dot::display_path(&target_path, false);
            anyhow::bail!("No tracked dotfile found at {}", display);
        }
        Ok(exact)
    } else {
        if dotfiles_in_path.is_empty() {
            let display = crate::dot::display_path(&target_path, false);
            anyhow::bail!("No tracked dotfiles found under {}", display);
        }
        Ok(dotfiles_in_path.into_iter().cloned().collect())
    }
}

fn pick_dotfiles_fzf(all_dotfiles: &DotfileMap) -> Result<Vec<crate::dot::Dotfile>> {
    let entries: Vec<DotfileEntry> = all_dotfiles
        .values()
        .map(|d| DotfileEntry {
            display: crate::dot::display_path(&d.target_path, d.is_root),
            target_path: d.target_path.clone(),
        })
        .collect();

    if entries.is_empty() {
        anyhow::bail!("No tracked dotfiles found");
    }

    let selection = FzfWrapper::builder()
        .multi_select(true)
        .prompt("Select dotfiles to delete")
        .header("Dotfiles")
        .select(entries)?;

    match selection {
        FzfResult::MultiSelected(items) => {
            let target_set: HashSet<_> = items.iter().map(|e| e.target_path.clone()).collect();
            Ok(all_dotfiles
                .values()
                .filter(|d| target_set.contains(&d.target_path))
                .cloned()
                .collect())
        }
        FzfResult::Selected(item) => Ok(all_dotfiles
            .values()
            .filter(|d| d.target_path == item.target_path)
            .cloned()
            .collect()),
        FzfResult::Cancelled => {
            emit(
                Level::Info,
                "dot.delete.cancelled",
                &format!("{} Cancelled", char::from(NerdFont::Info)),
                None,
            );
            Ok(vec![])
        }
        FzfResult::Error(err) => anyhow::bail!("fzf error: {err}"),
    }
}

fn delete_single_dotfile(db: &Database, dotfile: &crate::dot::Dotfile, debug: bool) -> Result<()> {
    let display = crate::dot::display_path(&dotfile.target_path, dotfile.is_root);

    // Delete target file from home directory
    match std::fs::remove_file(&dotfile.target_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    // Delete source file from repo
    let source_existed;
    match std::fs::remove_file(&dotfile.source_path) {
        Ok(()) => source_existed = true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => source_existed = false,
        Err(e) => return Err(e.into()),
    }

    if source_existed {
        // Clean up empty parent directories in repo
        clean_empty_parent_dirs(&dotfile.source_path, EmptyParentBoundary::HomeOrDots);

        // Stage the deletion in git
        if let Some(repo_path) = find_repo_root(&dotfile.source_path) {
            let relative = dotfile
                .source_path
                .strip_prefix(&repo_path)
                .unwrap_or(&dotfile.source_path);
            crate::dot::git::repo_ops::git_add(&repo_path, relative, debug)?;
        }
    }

    // Remove DB records for both source and target paths
    db.remove_hashes_for_path(&dotfile.source_path)?;
    db.remove_hashes_for_path(&dotfile.target_path)?;
    db.remove_managed_target(&dotfile.target_path)?;

    // Clean up empty parent directories in home
    if !dotfile.is_root {
        clean_empty_parent_dirs(&dotfile.target_path, EmptyParentBoundary::Home);
    }

    println!(
        "{} Deleted {}",
        char::from(NerdFont::Check),
        display.green()
    );

    Ok(())
}

/// Find the git repository root by walking up from a path looking for .git
fn find_repo_root(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = path.parent()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}
