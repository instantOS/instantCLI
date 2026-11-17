use crate::dot::config::{DotfileConfig, Repo};
use crate::dot::db::Database;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::repo::DotfileRepositoryManager;
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Build preview for a repository in the main menu
pub fn build_repo_preview(repo_name: &str, config: &DotfileConfig, db: &Database) -> String {
    let Some((index, repo_config)) = config
        .repos
        .iter()
        .enumerate()
        .find(|(_, r)| r.name == repo_name)
    else {
        return format!("Repository '{}' not found in config", repo_name);
    };
    let repo_info = DotfileRepositoryManager::new(config, db).get_repository_info(repo_name);
    render_repo_preview(repo_config, index + 1, config, &repo_info)
}

pub(super) fn render_repo_preview(
    repo_config: &Repo,
    priority: usize,
    config: &DotfileConfig,
    repo_info: &anyhow::Result<DotfileRepo>,
) -> String {
    let repo_name = &repo_config.name;
    let mut builder = PreviewBuilder::new().title(colors::SKY, repo_name).blank();

    // Show external repo status if applicable
    if repo_config.is_external() {
        builder = builder
            .line(
                colors::YELLOW,
                Some(NerdFont::Info),
                "External (Yadm/Stow compatible - metadata in config)",
            )
            .indented_line(colors::TEXT, None, "Fixed layout rooted at '.'")
            .indented_line(
                colors::TEXT,
                None,
                "No additional subdirs or root-owned '_root' directories",
            );
    }

    builder = builder.line(
        colors::TEXT,
        Some(NerdFont::Link),
        &format!("URL: {}", repo_config.url),
    );

    // Branch
    if let Some(branch) = &repo_config.branch {
        builder = builder.line(
            colors::TEXT,
            Some(NerdFont::GitBranch),
            &format!("Branch: {}", branch),
        );
    }

    // Priority
    let total_repos = config.repos.len();

    if priority > 0 {
        let label = if priority == 1 && total_repos > 1 {
            " (highest priority)"
        } else if priority == total_repos && total_repos > 1 {
            " (lowest priority)"
        } else {
            ""
        };

        builder = builder.line(
            colors::PEACH,
            Some(NerdFont::ArrowUp),
            &format!("Priority: P{}{}", priority, label),
        );
    }

    // Status
    let status_color = if repo_config.enabled {
        colors::GREEN
    } else {
        colors::RED
    };
    let status_text = if repo_config.enabled {
        "Enabled"
    } else {
        "Disabled"
    };
    let status_icon = if repo_config.enabled {
        NerdFont::ToggleOn
    } else {
        NerdFont::ToggleOff
    };
    builder = builder.line(status_color, Some(status_icon), status_text);

    // Read-only
    if repo_config.read_only {
        builder = builder.line(colors::YELLOW, Some(NerdFont::Lock), "Read-only");
    }

    // Try to get more info from DotfileRepo
    if let Ok(dotfile_repo) = repo_info {
        let subdir_summary = config.repo_subdir_summary(repo_config);
        let defaults_disabled = subdir_summary.defaults_disabled;

        // Show description if present
        if let Some(desc) = &dotfile_repo.meta.description {
            builder = builder.blank().line(
                colors::TEXT,
                Some(NerdFont::FileText),
                &format!("Description: {}", desc),
            );
        }

        // Show author if present
        if let Some(author) = &dotfile_repo.meta.author {
            builder = builder.line(
                colors::BLUE,
                Some(NerdFont::User),
                &format!("Author: {}", author),
            );
        }

        builder = builder
            .blank()
            .line(colors::MAUVE, Some(NerdFont::Folder), "Subdirectories");

        if defaults_disabled {
            builder = builder.indented_line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "Defaults disabled - enable subdirs to activate this repo",
            );
        }

        if dotfile_repo.meta.dots_dirs.is_empty() {
            builder = builder.indented_line(colors::SUBTEXT0, None, "No subdirectories configured");
        } else {
            let available = dotfile_repo.meta.dots_dirs.join(", ");
            let active = subdir_summary.active_label();
            builder = builder
                .indented_line(colors::TEXT, None, &format!("Available: {}", available))
                .indented_line(colors::GREEN, None, &format!("Active: {}", active));
        }

        // Show encryption status if present
        let recipients = &dotfile_repo.meta.encryption_recipients;
        if !recipients.is_empty() {
            builder =
                builder
                    .blank()
                    .line(colors::PEACH, Some(NerdFont::ShieldLock), "Age Encryption");

            let local_pubkeys =
                crate::dot::operations::key::get_local_public_keys().unwrap_or_default();
            let has_matching_identity = recipients.iter().any(|r| local_pubkeys.contains(r));

            let status_line = if has_matching_identity {
                format!("Authorized ({} recipient(s))", recipients.len())
            } else {
                format!(
                    "Unauthorized ({} recipient(s) - key missing)",
                    recipients.len()
                )
            };
            let status_color = if has_matching_identity {
                colors::GREEN
            } else {
                colors::RED
            };
            let status_icon = if has_matching_identity {
                NerdFont::Check
            } else {
                NerdFont::Warning
            };

            builder = builder.indented_line(status_color, Some(status_icon), &status_line);
        }

        // Local path
        if let Ok(local_path) = dotfile_repo.local_path(config) {
            let tilde_path = local_path.display().to_string();
            builder = builder.blank().indented_line(
                colors::TEXT,
                Some(NerdFont::Folder),
                &format!("Local: {}", tilde_path),
            );
        }
    } else if let Err(error) = repo_info {
        builder = builder.blank().line(
            colors::YELLOW,
            Some(NerdFont::Warning),
            &format!("Repository information unavailable: {error:#}"),
        );
    }

    builder.build_string()
}
