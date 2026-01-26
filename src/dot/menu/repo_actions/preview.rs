use crate::dot::config::Config;
use crate::dot::db::Database;
use crate::dot::repo::RepositoryManager;
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

/// Build preview for a repository in the main menu
pub fn build_repo_preview(repo_name: &str, config: &Config, db: &Database) -> String {
    let repo_manager = RepositoryManager::new(config, db);

    let repo_config = match config.repos.iter().find(|r| r.name == repo_name) {
        Some(rc) => rc,
        None => return format!("Repository '{}' not found in config", repo_name),
    };

    let mut builder = PreviewBuilder::new().title(colors::SKY, repo_name).blank();

    // Show external repo status if applicable
    if repo_config.metadata.is_some() {
        builder = builder.line(
            colors::YELLOW,
            Some(NerdFont::Info),
            "External (Yadm/Stow compatible - metadata in config)",
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
    let priority = config
        .repos
        .iter()
        .position(|r| r.name == repo_name)
        .map(|i| i + 1)
        .unwrap_or(0);
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

    // Try to get more info from LocalRepo
    if let Ok(local_repo) = repo_manager.get_repository_info(repo_name) {
        let defaults_disabled = repo_config.active_subdirectories.is_none()
            && local_repo
                .meta
                .default_active_subdirs
                .as_ref()
                .map(|dirs| dirs.is_empty())
                .unwrap_or(false);

        // Show description if present
        if let Some(desc) = &local_repo.meta.description {
            builder = builder.blank().line(
                colors::TEXT,
                Some(NerdFont::FileText),
                &format!("Description: {}", desc),
            );
        }

        // Show author if present
        if let Some(author) = &local_repo.meta.author {
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

        if local_repo.meta.dots_dirs.is_empty() {
            builder = builder.indented_line(colors::SUBTEXT0, None, "No subdirectories configured");
        } else {
            let available = local_repo.meta.dots_dirs.join(", ");
            let active = if let Some(active_subdirs) = &repo_config.active_subdirectories {
                if active_subdirs.is_empty() {
                    "(none configured)".to_string()
                } else {
                    active_subdirs.join(", ")
                }
            } else if local_repo.meta.dots_dirs.is_empty() {
                "(none configured)".to_string()
            } else {
                let repo_path = config.repos_path().join(&repo_config.name);
                let effective_active = config.resolve_active_subdirs(repo_config);
                if effective_active.is_empty() {
                    if defaults_disabled {
                        "(disabled by defaults)".to_string()
                    } else if repo_path.join("instantdots.toml").exists()
                        || repo_config.metadata.is_some()
                    {
                        "(none configured)".to_string()
                    } else {
                        "(none detected)".to_string()
                    }
                } else {
                    effective_active.join(", ")
                }
            };
            builder = builder
                .indented_line(colors::TEXT, None, &format!("Available: {}", available))
                .indented_line(colors::GREEN, None, &format!("Active: {}", active));
        }

        // Local path
        if let Ok(local_path) = local_repo.local_path(config) {
            let tilde_path = local_path.display().to_string();
            builder = builder.blank().indented_line(
                colors::TEXT,
                Some(NerdFont::Folder),
                &format!("Local: {}", tilde_path),
            );
        }
    }

    builder.build_string()
}
