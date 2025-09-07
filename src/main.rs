use clap::{Parser, Subcommand};

/// Instant — a tiny example CLI using clap
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
        None => {
            println!("instant: run with --help for usage");
        }
    }
}
