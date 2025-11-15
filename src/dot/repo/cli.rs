use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

#[derive(Subcommand, Debug)]
pub enum RepoCommands {
    /// List all configured repositories
    List,
    /// Clone a new repository (and immediately apply)
    Clone {
        url: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, short = 'b')]
        branch: Option<String>,
    },
    /// Remove a repository
    Remove {
        #[arg(add = ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
        #[arg(long)]
        keep_files: bool,
    },
    /// Show detailed repository information
    Info {
        #[arg(add = ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
    },
    /// Enable a disabled repository
    Enable {
        #[arg(add = ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
    },
    /// Disable a repository temporarily
    Disable {
        #[arg(add = ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
    },
    /// Subdirectory management
    Subdirs {
        #[command(subcommand)]
        command: SubdirCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum SubdirCommands {
    /// List available subdirectories
    List {
        #[arg(add = ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
        #[arg(long)]
        active: bool,
    },
    /// Set active subdirectories
    Set {
        #[arg(add = ArgValueCompleter::new(crate::completions::repo_name_completion))]
        name: String,
        subdirs: Vec<String>,
    },
}
