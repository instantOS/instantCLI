use clap::{Subcommand, ValueHint};

#[derive(Subcommand, Debug, Clone)]
pub enum ResolvethingCommands {
    /// Resolve duplicate files across configured scan directories
    #[command(alias = "dupes")]
    Duplicates {
        /// Restrict to a specific scan directory (path; ad-hoc if not configured)
        #[arg(long, value_hint = ValueHint::DirPath)]
        dir: Option<String>,
        /// Force interactive selection even when a safe automatic choice exists
        #[arg(long)]
        no_auto: bool,
        /// List the duplicate groups that were skipped (e.g. inside ignored folders)
        #[arg(long)]
        show_ignored: bool,
        /// Show what would be done without moving any files to trash
        #[arg(long)]
        dry_run: bool,
    },
    /// Resolve Syncthing conflict files across configured scan directories
    Conflicts {
        /// Restrict to a specific scan directory (path; ad-hoc if not configured)
        #[arg(long, value_hint = ValueHint::DirPath)]
        dir: Option<String>,
        /// Show what would be done without opening the diff editor or deleting files
        #[arg(long)]
        dry_run: bool,
    },
    /// Run duplicate and conflict resolution in sequence
    All {
        /// Restrict to a specific scan directory (path; ad-hoc if not configured)
        #[arg(long, value_hint = ValueHint::DirPath)]
        dir: Option<String>,
        /// Force interactive selection even when a safe automatic choice exists
        #[arg(long)]
        no_auto: bool,
        /// List the duplicate groups that were skipped (e.g. inside ignored folders)
        #[arg(long)]
        show_ignored: bool,
        /// Show what would be done without moving any files to trash or opening editors
        #[arg(long)]
        dry_run: bool,
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
    /// Print the current resolved configuration
    Show,
    /// Open the config file in your editor
    Edit,
}
