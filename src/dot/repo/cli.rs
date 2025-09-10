use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum RepoCommands {
    /// List all configured repositories
    List,
    /// Add a new repository (and immediately apply)
    Add { 
        url: String, 
        #[arg(long)]
        name: Option<String>, 
        #[arg(long, short = 'b')]
        branch: Option<String> 
    },
    /// Remove a repository
    Remove { 
        name: String, 
        #[arg(short, long)]
        files: bool 
    },
    /// Show detailed repository information
    Info { name: String },
    /// Enable a disabled repository
    Enable { name: String },
    /// Disable a repository temporarily
    Disable { name: String },
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
        name: String,
        #[arg(long)]
        active: bool 
    },
    /// Set active subdirectories
    Set { 
        name: String, 
        subdirs: Vec<String> 
    },
}