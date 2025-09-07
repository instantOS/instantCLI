mod dot;

use clap::{Parser, Subcommand};
use std::{env, fs, path::PathBuf};
use crate::dot::Repo;

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
}

fn main() {
    let cli = Cli::parse();

    if cli.debug {
        eprintln!("Debug mode is on");
    }

    match &cli.command {
        Some(Commands::Greet { name }) => {
            match name {
                Some(n) => println!("Hello, {}!", n),
                None => println!("Hello!"),
            }
        }
        Some(Commands::Dot { command }) => match command {
            DotCommands::Clone { repo, name, branch } => {
                let repo_obj = Repo {
                    url: repo.clone(),
                    name: name.clone(),
                    branch: branch.clone(),
                };
                match dot::add_repo(repo_obj, cli.debug) {
                    Ok(path) => println!("Added repo '{}' -> {}", repo, path.display()),
                    Err(e) => {
                        eprintln!("Error adding repo '{}': {}", repo, e);
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

fn basename_from_repo(repo: &str) -> String {
    // strip trailing .git if present
    let s = repo.trim_end_matches(".git");
    // split on '/' or ':' (to handle ssh-style URLs) and take last segment
    s.rsplit(|c| c == '/' || c == ':')
        .next()
        .map(|p| p.to_string())
        .unwrap_or_else(|| s.to_string())
}
