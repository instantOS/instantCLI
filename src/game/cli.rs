use clap::Subcommand;

/// Game save management commands
#[derive(Subcommand, Debug, Clone)]
pub enum GameCommands {
    /// Initialize restic repository for game saves
    Init {
        /// Restic repository path to use (non-interactive)
        #[arg(long)]
        repo: Option<String>,
        /// Restic repository password (defaults to built-in password)
        #[arg(long)]
        password: Option<String>,
    },
    /// Add a new game to track
    Add {
        /// Game name (non-interactive)
        #[arg(long)]
        name: Option<String>,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
        /// Optional launch command
        #[arg(long)]
        launch_command: Option<String>,
        /// Save path for the game (non-interactive)
        #[arg(long)]
        save_path: Option<String>,
        /// Create save path automatically when using --save-path
        #[arg(long)]
        create_save_path: bool,
    },
    /// Sync game saves with restic repository
    Sync {
        /// Game name to sync (optional, syncs all if not specified)
        game_name: Option<String>,
        /// Force sync even if checkpoint matches
        #[arg(long)]
        force: bool,
    },
    /// Launch a game with automatic save sync
    Launch {
        /// Game name to launch
        game_name: String,
    },
    /// List all configured games
    List,
    /// Show detailed information about a game
    Show {
        /// Game name to show (optional, will prompt if not specified)
        game_name: Option<String>,
    },
    /// Remove a game from tracking
    Remove {
        /// Game name to remove (optional, will prompt if not specified)
        game_name: Option<String>,
        /// Remove game without interactive confirmation
        #[arg(long)]
        force: bool,
    },
    /// Create a restic backup of game saves
    Backup {
        /// Game name to backup (optional, will prompt if not specified)
        game_name: Option<String>,
    },
    /// Run restic commands with instant games repository configuration
    Restic {
        /// Restic command and arguments to execute
        args: Vec<String>,
    },
    /// Restore game saves from a backup snapshot
    Restore {
        /// Game name to restore (optional, will prompt if not specified)
        game_name: Option<String>,
        /// Snapshot ID to restore from (optional, will prompt if not specified)
        snapshot_id: Option<String>,
        /// Force restore even if checkpoint matches
        #[arg(long)]
        force: bool,
    },
    /// Set up games that have been added but are not configured on this device
    Setup,
    /// Debug command: Show snapshot tag information (for developers)
    #[cfg(debug_assertions)]
    Debug {
        /// Debug subcommands
        #[command(subcommand)]
        debug_command: DebugCommands,
    },
}

/// Debug commands for developers
#[cfg(debug_assertions)]
#[derive(Subcommand, Debug, Clone)]
pub enum DebugCommands {
    /// Show detailed snapshot tag information
    Tags {
        /// Show tags for specific game (optional, shows all if not specified)
        game_name: Option<String>,
    },
}
