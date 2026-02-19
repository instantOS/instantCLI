use anyhow::Result;
use clap::{Subcommand, ValueHint};
use colored::Colorize;

use super::config::DotfileConfig;
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
        /// Show which repository/subdirectory each dotfile comes from
        #[arg(long)]
        show_sources: bool,
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
    /// Merge a modified dotfile with its source using nvim diff
    Merge {
        /// Path to the dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
        /// Show verbose output including unmodified files
        #[arg(long)]
        verbose: bool,
    },
    /// Manage ignored paths
    Ignore {
        #[command(subcommand)]
        command: IgnoreCommands,
    },
    /// Manage dotfile units
    Unit {
        /// Manage units in a repo (defaults to global units)
        #[arg(long, value_name = "REPO")]
        repo: Option<String>,
        #[command(subcommand)]
        command: UnitCommands,
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
    /// Pull changes in all writable repositories
    Pull {
        /// Arguments to pass to git pull (e.g. "--rebase")
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
        /// Path to the dotfile or directory (defaults to ~/ to browse all)
        #[arg(value_hint = ValueHint::AnyPath, default_value = "~")]
        path: String,
        /// Remove the override for this file (use default priority)
        #[arg(long)]
        reset: bool,
        /// Create the file in a new repo/subdir if it doesn't exist there
        #[arg(long)]
        create: bool,
        /// List available alternatives and exit
        #[arg(long)]
        list: bool,
        /// Set source to REPO or REPO/SUBDIR (non-interactive)
        #[arg(long, value_name = "REPO[/SUBDIR]")]
        set: Option<String>,
        /// Repository name (with --create for non-interactive mode)
        #[arg(long, requires = "create")]
        repo: Option<String>,
        /// Subdirectory name (with --create for non-interactive mode)
        #[arg(long, requires = "repo")]
        subdir: Option<String>,
    },
    /// Manage repository priority order
    Priority {
        #[command(subcommand)]
        command: PriorityCommands,
    },
    /// Interactive dotfile repository menu
    Menu {
        /// Open the menu in a GUI terminal window
        #[arg(long = "gui")]
        gui: bool,
    },
    /// Open a repository in lazygit (alias for 'repo lazygit')
    Lg {
        /// Repository name (optional, will prompt if not provided)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: Option<String>,
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

#[derive(Subcommand, Debug)]
pub enum UnitCommands {
    /// Add a unit directory (relative to ~)
    Add {
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
    },
    /// Remove a unit directory
    Remove {
        #[arg(value_hint = ValueHint::AnyPath)]
        path: String,
    },
    /// List configured unit directories
    List,
}

#[derive(Subcommand, Debug)]
pub enum PriorityCommands {
    /// Increase repository priority (move earlier in list)
    Bump {
        /// Repository name
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
    },
    /// Decrease repository priority (move later in list)
    Lower {
        /// Repository name
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
    },
    /// Set repository to a specific priority position
    Set {
        /// Repository name
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
        /// Priority position (1 = highest priority)
        position: usize,
    },
    /// List repositories in priority order
    List,
}

fn handle_ignore_command(
    config: &mut DotfileConfig,
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

fn handle_unit_command(
    config: &mut DotfileConfig,
    db: &Database,
    command: &UnitCommands,
    repo: Option<&str>,
    config_path: Option<&str>,
) -> Result<()> {
    let scope = match repo {
        Some(name) => crate::dot::unit_manager::UnitScope::Repo(name.to_string()),
        None => crate::dot::unit_manager::UnitScope::Global,
    };
    let context = crate::dot::unit_manager::unit_path_context_for_write(&scope, config, db)?;

    match command {
        UnitCommands::Add { path } => {
            let normalized_path = crate::dot::unit_manager::normalize_unit_input(path, &context)?;
            crate::dot::unit_manager::add_unit(&scope, config, db, &normalized_path, config_path)?;
            emit(
                Level::Success,
                "dot.unit.added",
                &format!(
                    "{} Added {} to units",
                    char::from(NerdFont::Check),
                    normalized_path.green()
                ),
                Some(serde_json::json!({
                    "path": normalized_path,
                    "action": "added",
                    "scope": scope_label(&scope)
                })),
            );
        }
        UnitCommands::Remove { path } => {
            let normalized_path = crate::dot::unit_manager::normalize_unit_input(path, &context)?;
            crate::dot::unit_manager::remove_unit(
                &scope,
                config,
                db,
                &normalized_path,
                config_path,
            )?;
            emit(
                Level::Success,
                "dot.unit.removed",
                &format!(
                    "{} Removed {} from units",
                    char::from(NerdFont::Check),
                    normalized_path.green()
                ),
                Some(serde_json::json!({
                    "path": normalized_path,
                    "action": "removed",
                    "scope": scope_label(&scope)
                })),
            );
        }
        UnitCommands::List => {
            let units = crate::dot::unit_manager::list_units(&scope, config, db)?;
            if units.is_empty() {
                emit(
                    Level::Info,
                    "dot.unit.list.empty",
                    &format!(
                        "{} No unit directories configured",
                        char::from(NerdFont::Info)
                    ),
                    None,
                );
            } else {
                emit(
                    Level::Info,
                    "dot.unit.list.header",
                    &format!("{} Unit directories:", char::from(NerdFont::List)),
                    Some(serde_json::json!({
                        "count": units.len(),
                        "scope": scope_label(&scope)
                    })),
                );
                for (i, unit) in units.iter().enumerate() {
                    emit(
                        Level::Info,
                        "dot.unit.list.item",
                        &format!("  {} {}", i + 1, unit.cyan()),
                        Some(serde_json::json!({
                            "index": i + 1,
                            "path": unit,
                            "scope": scope_label(&scope)
                        })),
                    );
                }
            }
        }
    }

    Ok(())
}

fn scope_label(scope: &crate::dot::unit_manager::UnitScope) -> String {
    scope.to_string()
}

fn handle_priority_command(
    config: &mut DotfileConfig,
    command: &PriorityCommands,
    config_path: Option<&str>,
) -> Result<()> {
    match command {
        PriorityCommands::Bump { name } => {
            let new_pos = config.move_repo_up(name, config_path)?;
            emit(
                Level::Success,
                "dot.priority.bump",
                &format!(
                    "{} Moved repository '{}' up to priority P{}",
                    char::from(NerdFont::Check),
                    name.cyan(),
                    new_pos
                ),
                Some(serde_json::json!({
                    "name": name,
                    "new_priority": new_pos
                })),
            );
        }
        PriorityCommands::Lower { name } => {
            let new_pos = config.move_repo_down(name, config_path)?;
            emit(
                Level::Success,
                "dot.priority.lower",
                &format!(
                    "{} Moved repository '{}' down to priority P{}",
                    char::from(NerdFont::Check),
                    name.cyan(),
                    new_pos
                ),
                Some(serde_json::json!({
                    "name": name,
                    "new_priority": new_pos
                })),
            );
        }
        PriorityCommands::Set { name, position } => {
            config.set_repo_priority(name, *position, config_path)?;
            emit(
                Level::Success,
                "dot.priority.set",
                &format!(
                    "{} Set repository '{}' to priority P{}",
                    char::from(NerdFont::Check),
                    name.cyan(),
                    position
                ),
                Some(serde_json::json!({
                    "name": name,
                    "priority": position
                })),
            );
        }
        PriorityCommands::List => {
            if config.repos.is_empty() {
                emit(
                    Level::Info,
                    "dot.priority.list.empty",
                    &format!("{} No repositories configured", char::from(NerdFont::Info)),
                    None,
                );
            } else {
                emit(
                    Level::Info,
                    "dot.priority.list.header",
                    &format!(
                        "{} Repository priority order (first = highest):",
                        char::from(NerdFont::List)
                    ),
                    None,
                );
                for (i, repo) in config.repos.iter().enumerate() {
                    let priority = format!("[P{}]", i + 1).bright_purple().bold();
                    let status = if repo.enabled {
                        "".to_string()
                    } else {
                        " (disabled)".yellow().to_string()
                    };
                    emit(
                        Level::Info,
                        "dot.priority.list.item",
                        &format!("  {} {}{}", priority, repo.name.cyan(), status),
                        Some(serde_json::json!({
                            "priority": i + 1,
                            "name": repo.name,
                            "enabled": repo.enabled
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
    let mut config = DotfileConfig::load(config_path)?;
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
        DotCommands::Status {
            path,
            all,
            show_sources,
        } => {
            super::status_all(&config, path.as_deref(), &db, *all, *show_sources)?;
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
        DotCommands::Merge { path, verbose } => {
            super::operations::merge::merge_dotfile(&config, &db, path, *verbose)?;
        }
        DotCommands::Ignore { command } => {
            handle_ignore_command(&mut config, command, config_path)?;
        }
        DotCommands::Unit { repo, command } => {
            handle_unit_command(&mut config, &db, command, repo.as_deref(), config_path)?;
        }
        DotCommands::Commit { args } => {
            super::git_commit_all(&config, args, debug)?;
        }
        DotCommands::Push { args } => {
            super::git_push_all(&config, args, debug)?;
        }
        DotCommands::Pull { args } => {
            let pulled_commits = super::git_pull_all(&config, args, debug)?;
            if pulled_commits {
                println!();
                super::status_all(&config, None, &db, false, false)?;
                println!(
                    "\n{} New commits were pulled. Use {} to apply the changes.",
                    char::from(NerdFont::Info).to_string().blue(),
                    "ins dot apply".green()
                );
            }
        }
        DotCommands::Git { args } => {
            super::git_run_any(&config, args, debug)?;
        }
        DotCommands::Alternative {
            path,
            reset,
            create,
            list,
            set,
            repo,
            subdir,
        } => {
            super::operations::alternative::handle_alternative(
                &config,
                super::operations::alternative::AlternativeOptions {
                    path,
                    reset: *reset,
                    create: *create,
                    list: *list,
                    set: set.as_deref(),
                    repo: repo.as_deref(),
                    subdir: subdir.as_deref(),
                },
            )?;
        }
        DotCommands::Priority { command } => {
            handle_priority_command(&mut config, command, config_path)?;
        }
        DotCommands::Menu { gui } => {
            if *gui {
                return crate::common::terminal::launch_menu_in_terminal(
                    "dot",
                    "Dotfile Menu",
                    &[],
                    debug,
                );
            }
            super::menu::dot_menu(debug)?;
        }
        DotCommands::Lg { name } => {
            super::repo::commands::interactive::open_repo_lazygit(&config, &db, name.as_deref())?;
        }
    }

    Ok(())
}
