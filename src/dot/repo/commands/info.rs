use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::repo::DotfileRepositoryManager;
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;
use colored::*;

/// Show detailed repository information
pub(super) fn show_repository_info(
    config: &DotfileConfig,
    db: &Database,
    name: &str,
) -> Result<()> {
    let repo_manager = DotfileRepositoryManager::new(config, db);

    let local_repo = repo_manager.get_repository_info(name)?;
    let repo_config = config
        .repos
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("Repository '{}' not found in configuration", name))?;

    let local_path = local_repo.local_path(config)?.display().to_string();
    let status_text = if repo_config.enabled {
        "Enabled".green().to_string()
    } else {
        "Disabled".yellow().to_string()
    };

    let read_only_text = if repo_config.read_only {
        "Yes".yellow().to_string()
    } else {
        "No".green().to_string()
    };

    let mut rows: Vec<(char, &str, String)> = vec![
        (
            char::from(NerdFont::FolderGit),
            "Repository",
            name.cyan().to_string(),
        ),
        (char::from(NerdFont::Link), "URL", repo_config.url.clone()),
        (
            char::from(NerdFont::GitBranch),
            "Branch",
            repo_config
                .branch
                .as_deref()
                .unwrap_or("default")
                .to_string(),
        ),
        (char::from(NerdFont::Check), "Status", status_text),
        (char::from(NerdFont::Lock), "Read-only", read_only_text),
        (char::from(NerdFont::Folder), "Local Path", local_path),
    ];

    if let Some(author) = &local_repo.meta.author {
        rows.push((char::from(NerdFont::User), "Author", author.clone()));
    }
    if let Some(description) = &local_repo.meta.description {
        rows.push((
            char::from(NerdFont::FileText),
            "Description",
            description.clone(),
        ));
    }

    let label_width = rows
        .iter()
        .map(|(_, label, _)| label.len())
        .max()
        .unwrap_or(0)
        + 1;

    println!();
    println!(
        "{} {}",
        char::from(NerdFont::List),
        "Repository Information".bold()
    );

    for (icon, label, value) in rows {
        println!(
            "  {} {:<width$} {}",
            icon,
            format!("{}:", label),
            value,
            width = label_width + 1
        );
    }

    println!();
    println!("{} {}", char::from(NerdFont::List), "Subdirectories".bold());

    let defaults_disabled = repo_config.active_subdirectories.is_none()
        && local_repo
            .meta
            .default_active_subdirs
            .as_ref()
            .map(|dirs| dirs.is_empty())
            .unwrap_or(false);

    if defaults_disabled {
        println!(
            "  {} {}",
            char::from(NerdFont::Warning),
            "Defaults disabled - repo inactive until you enable subdirs".yellow()
        );
    }

    if local_repo.dotfile_dirs.is_empty() {
        println!(
            "  {} {}",
            char::from(NerdFont::Info),
            "No dotfile directories discovered.".dimmed()
        );
        return Ok(());
    }

    let dir_name_width = local_repo
        .dotfile_dirs
        .iter()
        .map(|dir| {
            dir.path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .len()
        })
        .max()
        .unwrap_or(0);

    for dir in &local_repo.dotfile_dirs {
        let dir_name = dir
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let status_icon = if dir.is_active {
            char::from(NerdFont::Check)
        } else {
            char::from(NerdFont::CrossCircle)
        };
        let status_text = if dir.is_active {
            "Active".green().to_string()
        } else {
            "Inactive".yellow().to_string()
        };
        let configured = repo_config
            .active_subdirectories
            .as_ref()
            .map(|subdirs| subdirs.contains(&dir_name))
            .unwrap_or(false);
        let configured_label = if configured {
            "configured".blue().to_string()
        } else {
            "not configured".dimmed().to_string()
        };

        println!(
            "  {} {:<name_width$} {}  ({})  {}",
            status_icon,
            dir_name,
            status_text,
            configured_label,
            dir.path.display(),
            name_width = dir_name_width + 2
        );
    }

    // Warn about orphaned subdirs (enabled in config but not in metadata)
    let orphaned = local_repo.get_orphaned_active_subdirs(config);
    if !orphaned.is_empty() {
        let repo_path = local_repo.local_path(config)?;
        let metadata_path = repo_path.join("instantdots.toml");
        println!();
        println!(
            "{} {}",
            char::from(NerdFont::Warning),
            "Orphaned Subdirectories".bold().yellow()
        );
        for subdir in &orphaned {
            println!(
                "  {} '{}' is enabled but not in metadata",
                char::from(NerdFont::Warning).to_string().yellow(),
                subdir
            );
            println!(
                "     Fix: {} or add '{}' to {}",
                format!("ins dot repo subdirs disable {} {}", name, subdir).dimmed(),
                subdir,
                metadata_path.display()
            );
        }
    }

    Ok(())
}
