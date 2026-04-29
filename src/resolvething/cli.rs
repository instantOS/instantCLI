use clap::{Subcommand, ValueHint};

#[derive(Subcommand, Debug, Clone)]
pub enum ResolvethingCommands {
    /// Resolve duplicate files under the configured working directory
    #[command(alias = "dupes")]
    Duplicates {
        /// Override the configured working directory for this run
        #[arg(long, value_hint = ValueHint::DirPath)]
        path: Option<String>,
        /// Force interactive selection even when a safe automatic choice exists
        #[arg(long)]
        no_auto: bool,
    },
    /// Resolve Syncthing conflict files under the configured working directory
    Conflicts {
        /// Override the configured working directory for this run
        #[arg(long, value_hint = ValueHint::DirPath)]
        path: Option<String>,
        /// Restrict conflict resolution to specific file extensions
        #[arg(long = "type", value_delimiter = ',')]
        types: Vec<String>,
    },
    /// Run duplicate and conflict resolution in sequence
    All {
        /// Override the configured working directory for this run
        #[arg(long, value_hint = ValueHint::DirPath)]
        path: Option<String>,
        /// Force interactive selection even when a safe automatic choice exists
        #[arg(long)]
        no_auto: bool,
        /// Restrict conflict resolution to specific file extensions
        #[arg(long = "type", value_delimiter = ',')]
        types: Vec<String>,
    },
    /// Interactive resolvething menu
    Menu {
        /// Open the menu in a GUI terminal window
        #[arg(long = "gui")]
        gui: bool,
    },
    /// Inspect or edit resolvething configuration
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommands>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommands {
    /// Print the active config file path
    Path,
    /// Print the current resolved configuration
    Show,
    /// Open the config file in your editor
    Edit,
}
