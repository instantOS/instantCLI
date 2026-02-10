use crate::common::TildePath;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::repo::DotfileRepositoryManager;
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;
use colored::*;

/// Show git repository status (working directory and branch sync state)
pub(super) fn show_repository_status(
    config: &DotfileConfig,
    db: &Database,
    name: Option<&str>,
) -> Result<()> {
    let repo_manager = DotfileRepositoryManager::new(config, db);

    // Determine which repos to show
    let repos_to_show: Vec<_> = if let Some(name) = name {
        vec![name.to_string()]
    } else {
        config.repos.iter().map(|r| r.name.clone()).collect()
    };

    for repo_name in repos_to_show {
        let local_repo = match repo_manager.get_repository_info(&repo_name) {
            Ok(repo) => repo,
            Err(e) => {
                eprintln!(
                    "{} {}: {}",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan(),
                    e
                );
                continue;
            }
        };

        let repo_path = local_repo.local_path(config)?;

        let git_repo = match git2::Repository::open(&repo_path) {
            Ok(repo) => repo,
            Err(e) => {
                eprintln!(
                    "{} {}: Failed to open git repository: {}",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan(),
                    e
                );
                continue;
            }
        };

        let repo_status = match crate::common::git::get_repo_status(&git_repo) {
            Ok(status) => status,
            Err(e) => {
                eprintln!(
                    "{} {}: Failed to get repo status: {}",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan(),
                    e
                );
                continue;
            }
        };

        let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
            Some(config) => config,
            None => {
                eprintln!(
                    "{} {}: Repository not found in configuration",
                    char::from(NerdFont::CrossCircle),
                    repo_name.cyan()
                );
                continue;
            }
        };

        let tilde_path = TildePath::new(repo_path.to_path_buf());
        let local_path = tilde_path
            .to_tilde_string()
            .unwrap_or_else(|_| repo_path.display().to_string());

        println!();
        println!(
            "{} {}",
            char::from(NerdFont::FolderGit),
            repo_name.bold().cyan()
        );

        // Working directory status
        let (icon, status_text) = if repo_status.working_dir_clean {
            (char::from(NerdFont::CheckCircle), "Clean".green())
        } else {
            (
                char::from(NerdFont::Edit),
                format!(
                    "Dirty [{} modified, {} untracked]",
                    repo_status.file_counts.modified, repo_status.file_counts.untracked
                )
                .yellow(),
            )
        };

        println!("  Working Directory:  {} {}", icon, status_text);

        // Branch sync status
        let (icon, _status_text) = match &repo_status.branch_sync {
            crate::common::git::BranchSyncStatus::UpToDate => {
                (char::from(NerdFont::Check), "Up-to-date".green())
            }
            crate::common::git::BranchSyncStatus::Ahead { commits } => (
                char::from(NerdFont::CloudUpload),
                format!("Ahead {} commits", commits).blue(),
            ),
            crate::common::git::BranchSyncStatus::Behind { commits } => (
                char::from(NerdFont::CloudDownload),
                format!("Behind {} commits", commits).blue(),
            ),
            crate::common::git::BranchSyncStatus::Diverged { ahead, behind } => (
                char::from(NerdFont::GitMerge),
                format!("Diverged ({} ahead, {} behind)", ahead, behind).red(),
            ),
            crate::common::git::BranchSyncStatus::NoRemote => {
                (char::from(NerdFont::Warning), "No remote".yellow())
            }
        };

        println!(
            "  Branch Status:       {} ({})",
            icon,
            repo_status.branch.dimmed()
        );
        println!("  URL:                 {}", repo_config.url.dimmed());
        println!("  Local Path:          {}", local_path.dimmed());
    }

    println!();

    Ok(())
}
