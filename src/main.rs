use colored::*;

mod dot;

use clap::{Parser, Subcommand};

use crate::dot::config::Repo;

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
    /// Greet someone
    Greet { name: Option<String> },

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
    /// Reset modified dotfiles to their original state
    Reset,
    /// Apply dotfiles
    Apply,
    /// Fetch modified dotfiles
    Fetch,
    /// Pull updates for all configured repos
    Update,
    /// Check each configured repo's git status
    Status,
    /// Initialize the repo in the current directory as an instantdots repo
    Init {
        /// Optional name to set in instantdots.toml
        name: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    if cli.debug {
        eprintln!("Debug mode is on");
    }

    match &cli.command {
        Some(Commands::Greet { name }) => match name {
            Some(n) => println!("Hello, {}!", n),
            None => println!("Hello!"),
        },
        Some(Commands::Dot { command }) => match command {
            DotCommands::Clone { repo, name, branch } => {
                let repo_obj = Repo {
                    url: repo.clone(),
                    name: name.clone(),
                    branch: branch.clone(),
                };
                match dot::add_repo(repo_obj.into(), cli.debug) {
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
            DotCommands::Reset => {
                let db = dot::db::Database::new().unwrap();
                let filemap = dot::get_all_dotfiles().unwrap();
                for dotfile in filemap.values() {
                    if dotfile.is_modified(&db) {
                        dotfile.apply(&db).unwrap();
                    }
                }
                db.cleanup_hashes().unwrap();
            }
            DotCommands::Apply => {
                let db = dot::db::Database::new().unwrap();
                let filemap = dot::get_all_dotfiles().unwrap();
                for dotfile in filemap.values() {
                    dotfile.apply(&db).unwrap();
                }
            }
            DotCommands::Fetch => {
                let db = dot::db::Database::new().unwrap();
                let filemap = dot::get_all_dotfiles().unwrap();
                for dotfile in filemap.values() {
                    dotfile.fetch(&db).unwrap();
                }
                db.cleanup_hashes().unwrap();
            }
            DotCommands::Update => {
                let db = dot::db::Database::new().unwrap();
                match dot::update_all(cli.debug) {
                    Ok(()) => println!("{}", "All repos updated".green()),
                    Err(e) => {
                        eprintln!("{}: {}", "Error updating repos".red(), e.to_string().red());
                        std::process::exit(1);
                    }
                }
                db.cleanup_hashes().unwrap();
            }
            DotCommands::Status => match dot::status_all(cli.debug) {
                Ok(()) => (),
                Err(e) => {
                    eprintln!("Error checking repo status: {}", e);
                    std::process::exit(1);
                }
            },
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
        },
        None => {
            println!("instant: run with --help for usage");
        }
    }
}
