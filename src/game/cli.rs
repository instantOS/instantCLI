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
}