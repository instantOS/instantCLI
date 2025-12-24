use anyhow::Result;
use clap::{Subcommand, ValueHint};
use colored::Colorize;

use super::config::Config;
use super::db::Database;
use super::repo::cli::{CloneArgs, RepoCommands};
use crate::ui::prelude::*;

#[derive(Subcommand, Debug)]
pub enum DotCommands {
    /// Repository management commands
    Repo {
        #[command(subcommand)]
        command: RepoCommands,
    },
    /// Clone a new repository (alias for 'repo clone')
    Clone(CloneArgs),
    /// Reset modified dotfiles to their original state in the given path
    Reset {
        /// Path to reset (relative to ~)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
    },
    /// Apply dotfiles
    Apply,
    /// Add or update dotfiles
    ///
    /// For a single file: If tracked, update the source file. If untracked, prompt to add it.
    /// For a directory: Update all tracked files. Use --all to also add untracked files.
    Add {
        /// Path to add or update (relative to ~)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
        /// Recursively add all files in directory, including untracked ones
        #[arg(long)]
        all: bool,
        /// Choose which repository/subdirectory to add the file to
        #[arg(long)]
        choose: bool,
    },
    /// Pull updates for all configured repos and apply changes
    Update {
        /// Do not apply dotfiles after updating
        #[arg(long)]
        no_apply: bool,
    },
    /// Check dotfile status
    Status {
        /// Optional path to a dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: Option<String>,
        /// Show all dotfiles including clean ones
        #[arg(long)]
        all: bool,
    },
    /// Initialize the current git repo or bootstrap a default dotfile repo when outside git
    Init {
        /// Optional name to set in instantdots.toml
        name: Option<String>,
        /// Run non-interactively (use provided name or directory name)
        #[arg(long)]
        non_interactive: bool,
    },
    /// Show differences between modified dotfiles and their source
    Diff {
        /// Optional path to a dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: Option<String>,
    },
    /// Manage ignored paths
    Ignore {
        #[command(subcommand)]
        command: IgnoreCommands,
    },
    /// Commit changes in all writable repositories
    Commit {
        /// Arguments to pass to git commit (e.g. "-m 'message'")
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Push changes in all writable repositories
    Push {
        /// Arguments to pass to git push (e.g. "origin main")
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run an arbitrary git command in a repository
    Git {
        /// Git command and arguments (e.g. "log --oneline")
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Set or view which repository/subdirectory a dotfile is sourced from
    Alternative {
        /// Path to the dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
        /// Remove the override for this file (use default priority)
        #[arg(long)]
        reset: bool,
        /// Create the file in a new repo/subdir if it doesn't exist there
        #[arg(long)]
        create: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum IgnoreCommands {
    /// Add a path to the ignore list
    Add {
        /// Path to ignore (relative to ~, e.g., .config/nvim or .bashrc)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
    },
    /// Remove a path from the ignore list
    Remove {
        /// Path to stop ignoring
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
    },
    /// List all ignored paths
    List,
}

fn handle_ignore_command(
    config: &mut Config,
    command: &IgnoreCommands,
    config_path: Option<&str>,
) -> Result<()> {
    match command {
        IgnoreCommands::Add { path } => {
            let normalized_path = if path.starts_with('~') {
                path.clone()
            } else if path.starts_with('/') {
                let home = shellexpand::tilde("~").to_string();
                if path.starts_with(&home) {
                    format!("~{}", path.strip_prefix(&home).unwrap_or(path))
                } else {
                    return Err(anyhow::anyhow!(
                        "Path must be within home directory. Use ~ or relative paths."
                    ));
                }
            } else {
                format!("~/{}", path.trim_start_matches('/'))
            };

            config.add_ignored_path(normalized_path.clone(), config_path)?;
            emit(
                Level::Success,
                "dot.ignore.added",
                &format!(
                    "{} Added {} to ignore list",
                    char::from(NerdFont::Check),
                    normalized_path.green()
                ),
                Some(serde_json::json!({
                    "path": normalized_path,
                    "action": "added"
                })),
            );
        }
        IgnoreCommands::Remove { path } => {
            let normalized_path = if path.starts_with('~') {
                path.clone()
            } else if path.starts_with('/') {
                let home = shellexpand::tilde("~").to_string();
                if path.starts_with(&home) {
                    format!("~{}", path.strip_prefix(&home).unwrap_or(path))
                } else {
                    return Err(anyhow::anyhow!(
                        "Path must be within home directory. Use ~ or relative paths."
                    ));
                }
            } else {
                format!("~/{}", path.trim_start_matches('/'))
            };

            config.remove_ignored_path(&normalized_path, config_path)?;
            emit(
                Level::Success,
                "dot.ignore.removed",
                &format!(
                    "{} Removed {} from ignore list",
                    char::from(NerdFont::Check),
                    normalized_path.green()
                ),
                Some(serde_json::json!({
                    "path": normalized_path,
                    "action": "removed"
                })),
            );
        }
        IgnoreCommands::List => {
            if config.ignored_paths.is_empty() {
                emit(
                    Level::Info,
                    "dot.ignore.list.empty",
                    &format!(
                        "{} No paths are currently ignored",
                        char::from(NerdFont::Info)
                    ),
                    None,
                );
            } else {
                emit(
                    Level::Info,
                    "dot.ignore.list.header",
                    &format!("{} Ignored paths:", char::from(NerdFont::List)),
                    Some(serde_json::json!({
                        "count": config.ignored_paths.len()
                    })),
                );
                for (i, path) in config.ignored_paths.iter().enumerate() {
                    emit(
                        Level::Info,
                        "dot.ignore.list.item",
                        &format!("  {} {}", (i + 1), path.cyan()),
                        Some(serde_json::json!({
                            "index": i + 1,
                            "path": path
                        })),
                    );
                }
            }
        }
    }

    Ok(())
}

pub fn handle_dot_command(
    command: &DotCommands,
    config_path: Option<&str>,
    debug: bool,
) -> Result<()> {
    let mut config = Config::load(config_path)?;
    config.ensure_directories()?;
    let db = Database::new(config.database_path().to_path_buf())?;

    match command {
        DotCommands::Repo { command } => {
            super::repo::commands::handle_repo_command(&mut config, &db, command, debug)?;
        }
        DotCommands::Clone(args) => {
            super::repo::commands::handle_repo_command(
                &mut config,
                &db,
                &RepoCommands::Clone(args.clone()),
                debug,
            )?;
        }
        DotCommands::Reset { path } => {
            super::reset_modified(&config, &db, path)?;
        }
        DotCommands::Apply => {
            super::apply_all(&config, &db)?;
        }
        DotCommands::Add { path, all, choose } => {
            super::add_dotfile(&config, &db, path, *all, *choose, debug)?;
        }
        DotCommands::Update { no_apply } => {
            super::update_all(&config, debug, &db, !*no_apply)?;
        }
        DotCommands::Status { path, all } => {
            super::status_all(&config, path.as_deref(), &db, *all)?;
        }
        DotCommands::Init {
            name,
            non_interactive,
        } => {
            let cwd = std::env::current_dir()
                .map_err(|e| anyhow::anyhow!("Unable to determine current directory: {}", e))?;
            super::meta::handle_init_command(&mut config, &cwd, name.as_deref(), *non_interactive)?;
        }
        DotCommands::Diff { path } => {
            super::diff_all(&config, path.as_deref(), &db)?;
        }
        DotCommands::Ignore { command } => {
            handle_ignore_command(&mut config, command, config_path)?;
        }
        DotCommands::Commit { args } => {
            super::git_commit_all(&config, args, debug)?;
        }
        DotCommands::Push { args } => {
            super::git_push_all(&config, args, debug)?;
        }
        DotCommands::Git { args } => {
            super::git_run_any(&config, args, debug)?;
        }
        DotCommands::Alternative { path, reset, create } => {
            super::operations::alternative::handle_alternative(&config, path, *reset, *create)?;
        }
    }

    Ok(())
}
