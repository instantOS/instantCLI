use clap::Subcommand;

/// Game save management commands
#[derive(Subcommand, Debug, Clone)]
pub enum GameCommands {
    /// Initialize restic repository for game saves
    Init,
    /// Add a new game to track
    Add,
    /// Sync game saves with restic repository
    Sync {
        /// Game name to sync (optional, syncs all if not specified)
        game_name: Option<String>,
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
    },
}
