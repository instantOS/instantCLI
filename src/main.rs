use anyhow::Result;
use colored::*;

mod dot;

use clap::{Parser, Subcommand};

use crate::dot::config::{Config, Repo, extract_repo_name};
use crate::dot::db::Database;

/// InstantCLI main parser
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Activate debug mode
    #[arg(short, long, global = true)]
    debug: bool,

    /// Custom config file path
    #[arg(short = 'c', long = "config", global = true)]
    config: Option<String>,

    /// Custom database file path
    #[arg(long = "database", global = true)]
    database: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Dotfile management commands
    Dot {
        #[command(subcommand)]
        command: DotCommands,
    },
}

#[derive(Subcommand, Debug)]
enum DotCommands {
    /// Clone a dotfiles repo into the local store
    Clone {
        /// Repository URL to clone
        repo: String,
        /// Optional name to use for the repo directory (defaults to repo basename)
        #[arg(short, long)]
        name: Option<String>,
        /// Optional branch to checkout during clone
        #[arg(short = 'b', long = "branch")]
        branch: Option<String>,
    },
    /// Reset modified dotfiles to their original state in the given path
    Reset {
        /// Path to reset (relative to ~)
        path: String,
    },
    /// Apply dotfiles
    Apply,
    /// Fetch modified dotfiles from home directory back to repository
    Fetch {
        /// Path to fetch (relative to ~)
        path: Option<String>,
        /// Perform a dry run, showing which files would be fetched
        #[arg(long)]
        dry_run: bool,
    },
    /// Add new dotfiles to tracking
    Add {
        /// Path to add (relative to ~)
        path: String,
    },
    /// Pull updates for all configured repos
    Update,
    /// Check each configured repo's git status
    Status {
        /// Optional path to a dotfile (target path, e.g. ~/.config/kitty/kitty.conf)
        path: Option<String>,
    },
    /// Initialize the repo in the current directory as an instantdots repo
    Init {
        /// Optional name to set in instantdots.toml
        name: Option<String>,
        /// Run non-interactively (use provided name or directory name)
        #[arg(long)]
        non_interactive: bool,
    },
    /// List available subdirectories in a repo
    ListSubdirs {
        /// Repository name or URL
        repo: String,
    },
    /// Set active subdirectories for a repo
    SetSubdirs {
        /// Repository name or URL
        repo: String,
        /// Subdirectories to activate (space-separated)
        subdirs: Vec<String>,
    },
    /// Show active subdirectories for a repo
    ShowSubdirs {
        /// Repository name or URL
        repo: String,
    },
    /// Remove a repository from configuration
    Remove {
        /// Repository name to remove
        repo: String,
        /// Whether to also remove local files (default: false)
        #[arg(short, long)]
        files: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.debug {
        eprintln!("Debug mode is on");
    }

    // Load configuration once at startup
    let mut config = match Config::load_from(cli.config.as_deref()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!(
                "{}: {}",
                "Error loading configuration".red(),
                e.to_string().red()
            );
            return Err(e);
        }
    };

    // Create database instance once at startup
    let db = match Database::new(dot::config::db_path(cli.database.as_deref())?) {
        Ok(db) => db,
        Err(e) => {
            eprintln!(
                "{}: {}",
                "Error opening database".red(),
                e.to_string().red()
            );
            return Err(e);
        }
    };

    match &cli.command {
        Some(Commands::Dot { command }) => match command {
            DotCommands::Clone { repo, name, branch } => {
                let repo_name = name.clone().unwrap_or_else(|| extract_repo_name(&repo));
                let repo_obj = Repo {
                    url: repo.clone(),
                    name: repo_name,
                    branch: branch.clone(),
                    active_subdirectories: Vec::new(), // Will be set to default by config
                };
                match dot::add_repo(&mut config, repo_obj.into(), cli.debug) {
                    Ok(path) => println!(
                        "{} {} {} {}",
                        "Added repo".green(),
                        repo.green().bold(),
                        "->".green(),
                        path.display()
                    ),
                    Err(e) => {
                        eprintln!(
                            "{} {} {}",
                            "Error adding repo".red(),
                            repo.red().bold(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
            DotCommands::Reset { path } => match dot::reset_modified(&config, &db, &path) {
                Ok(()) => println!("{} {}", "Reset modified dotfiles in".green(), path.green()),
                Err(e) => {
                    eprintln!(
                        "{}: {}",
                        "Error resetting dotfiles".red(),
                        e.to_string().red()
                    );
                    return Err(e);
                }
            },
            DotCommands::Apply => match dot::apply_all(&config, &db) {
                Ok(()) => println!("{}", "Applied dotfiles".green()),
                Err(e) => {
                    eprintln!(
                        "{}: {}",
                        "Error applying dotfiles".red(),
                        e.to_string().red()
                    );
                    return Err(e);
                }
            },
            DotCommands::Fetch { path, dry_run } => {
                match dot::fetch_modified(&config, &db, path.as_deref(), *dry_run) {
                    Ok(()) => println!("{}", "Fetched modified dotfiles".green()),
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error fetching dotfiles".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
            DotCommands::Add { path } => match dot::add_dotfile(&config, &db, &path) {
                Ok(()) => println!("{} {}", "Added dotfile".green(), path.green()),
                Err(e) => {
                    eprintln!("{}: {}", "Error adding dotfile".red(), e.to_string().red());
                    return Err(e);
                }
            },
            DotCommands::Update => match dot::update_all(&config, cli.debug) {
                Ok(()) => println!("{}", "All repos updated".green()),
                Err(e) => {
                    eprintln!("{}: {}", "Error updating repos".red(), e.to_string().red());
                    return Err(e);
                }
            },
            DotCommands::Status { path } => {
                match dot::status_all(&config, cli.debug, path.as_deref(), &db) {
                    Ok(()) => (),
                    Err(e) => {
                        eprintln!("Error checking repo status: {}", e);
                        return Err(e);
                    }
                }
            }
            DotCommands::Init { name, non_interactive } => {
                let cwd = std::env::current_dir().expect("unable to determine cwd");
                match dot::meta::init_repo(&cwd, name.as_deref(), *non_interactive) {
                    Ok(()) => println!(
                        "{} {}",
                        "Initialized instantdots.toml in".green(),
                        cwd.display()
                    ),
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error initializing repo".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
            DotCommands::ListSubdirs { repo } => match dot::list_repo_subdirs(&config, &repo) {
                Ok(subdirs) => {
                    println!("Available subdirectories for {}:", repo.green());
                    for subdir in subdirs {
                        println!("  - {}", subdir);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{}: {}",
                        "Error listing subdirectories".red(),
                        e.to_string().red()
                    );
                    return Err(e);
                }
            },
            DotCommands::SetSubdirs { repo, subdirs } => {
                match dot::set_repo_active_subdirs(&mut config, &repo, subdirs.clone()) {
                    Ok(()) => println!(
                        "{} {} for {}",
                        "Set active subdirectories".green(),
                        subdirs.join(", ").green(),
                        repo.green()
                    ),
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error setting active subdirectories".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
            DotCommands::ShowSubdirs { repo } => {
                match dot::show_repo_active_subdirs(&config, &repo) {
                    Ok(subdirs) => {
                        println!("Active subdirectories for {}:", repo.green());
                        for subdir in subdirs {
                            println!("  - {}", subdir);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error showing active subdirectories".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
            DotCommands::Remove { repo, files } => {
                match dot::remove_repo(&mut config, &repo, *files) {
                    Ok(()) => println!("{} {}", "Removed repository".green(), repo.green()),
                    Err(e) => {
                        eprintln!(
                            "{}: {}",
                            "Error removing repository".red(),
                            e.to_string().red()
                        );
                        return Err(e);
                    }
                }
            }
        },
        None => {
            println!("instant: run with --help for usage");
        }
    }
    Ok(())
}
