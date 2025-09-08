use colored::*;

mod dot;

use clap::{Parser, Subcommand};

use crate::dot::config::{Config, Repo, basename_from_repo};

/// InstantCLI main parser
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Activate debug mode
    #[arg(short, long, global = true)]
    debug: bool,

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

fn main() {
    let cli = Cli::parse();

    if cli.debug {
        eprintln!("Debug mode is on");
    }

    // Load configuration once at startup
    let mut config = match Config::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!(
                "{}: {}",
                "Error loading configuration".red(),
                e.to_string().red()
            );
            std::process::exit(1);
        }
    };

    match &cli.command {
        Some(Commands::Dot { command }) => match command {
            DotCommands::Clone { repo, name, branch } => {
                let repo_name = name.clone().unwrap_or_else(|| basename_from_repo(&repo));
                let repo_obj = Repo {
                    url: repo.clone(),
                    name: repo_name,
                    branch: branch.clone(),
                    active_subdirs: Vec::new(), // Will be set to default by config
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
                        std::process::exit(1);
                    }
                }
            }
            DotCommands::Reset { path } => match dot::reset_modified(&config, &path) {
                Ok(()) => println!("{} {}", "Reset modified dotfiles in".green(), path.green()),
                Err(e) => {
                    eprintln!(
                        "{}: {}",
                        "Error resetting dotfiles".red(),
                        e.to_string().red()
                    );
                    std::process::exit(1);
                }
            },
            DotCommands::Apply => match dot::apply_all(&config) {
                Ok(()) => println!("{}", "Applied dotfiles".green()),
                Err(e) => {
                    eprintln!(
                        "{}: {}",
                        "Error applying dotfiles".red(),
                        e.to_string().red()
                    );
                    std::process::exit(1);
                }
            },
            DotCommands::Fetch { path } => match dot::fetch_modified(&config, path.as_deref()) {
                Ok(()) => println!("{}", "Fetched modified dotfiles".green()),
                Err(e) => {
                    eprintln!(
                        "{}: {}",
                        "Error fetching dotfiles".red(),
                        e.to_string().red()
                    );
                    std::process::exit(1);
                }
            },
            DotCommands::Add { path } => match dot::add_dotfile(&config, &path) {
                Ok(()) => println!("{} {}", "Added dotfile".green(), path.green()),
                Err(e) => {
                    eprintln!("{}: {}", "Error adding dotfile".red(), e.to_string().red());
                    std::process::exit(1);
                }
            },
            DotCommands::Update => match dot::update_all(&config, cli.debug) {
                Ok(()) => println!("{}", "All repos updated".green()),
                Err(e) => {
                    eprintln!("{}: {}", "Error updating repos".red(), e.to_string().red());
                    std::process::exit(1);
                }
            },
            DotCommands::Status { path } => {
                match dot::status_all(&config, cli.debug, path.as_deref()) {
                    Ok(()) => (),
                    Err(e) => {
                        eprintln!("Error checking repo status: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            DotCommands::Init { name } => {
                let cwd = std::env::current_dir().expect("unable to determine cwd");
                match dot::meta::init_repo(&cwd, name.as_deref()) {
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
                        std::process::exit(1);
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
                    std::process::exit(1);
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
                        std::process::exit(1);
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
                        std::process::exit(1);
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
                        std::process::exit(1);
                    }
                }
            }
        },
        None => {
            println!("instant: run with --help for usage");
        }
    }
}
