use std::ffi::OsString;

use clap::{Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCompleter;

/// Game save management commands
#[derive(Subcommand, Debug, Clone)]
pub enum GameCommands {
    /// Manage shared save ownership and device-specific bindings
    #[command(
        long_about = "Manage shared save ownership and device-specific bindings.\n\nEach group owns exactly one file or directory and one backup history. For PS2, select an individual file card or folder card; a parent directory is an explicit choice that rolls back every contained card together. Configure PCSX2 to use the declared card; ins does not infer card ownership from emulator settings.\n\nGroup IDs are readable names (for example shared-card). The same ID selects the same repository history on another device; renaming the display name preserves it, changing the ID does not. Group declarations and game membership live in games.toml; paths and sync state live in installations.toml. These config files are not automatically synchronized. Declare the same ID and membership, then bind its device-specific path on each device."
    )]
    Group {
        #[command(subcommand)]
        command: SaveGroupCommands,
    },
    /// Explicitly choose local saves or a backup after a conflict or migration
    Reconcile {
        game_name: String,
        /// Accept local data and publish it as the chosen version
        #[arg(long, conflicts_with = "snapshot")]
        local: bool,
        /// Restore this owner's snapshot and acknowledge the current remote head
        #[arg(long)]
        snapshot: Option<String>,
    },
    /// Initialize restic repository for game saves
    Init {
        /// Restic repository path to use (non-interactive)
        #[arg(long)]
        repo: Option<String>,
        /// Restic repository password (defaults to built-in password)
        #[arg(long)]
        password: Option<String>,
        /// Connect an existing repository instead of creating one (with --repo)
        #[arg(long)]
        existing: bool,
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
        /// Shared save group instead of a private save path
        #[arg(long, requires = "name", conflicts_with = "save_path")]
        save_group: Option<String>,
        /// Create save path automatically when using --save-path
        #[arg(long)]
        create_save_path: bool,
        /// Disable the discovered-games cache for this run
        ///
        /// Cache file: `$XDG_CACHE_HOME/instant/game-discovery-cache.json`
        /// (usually `~/.cache/instant/game-discovery-cache.json`)
        #[arg(long)]
        no_cache: bool,
    },
    /// Discover games available for `ins game add`
    Discover {
        /// Restrict discovery to a specific Wine prefix path or path inside one
        path: Option<String>,
        /// Limit discovery to specific platforms, e.g. `--source ps2 --source epic`
        #[arg(long = "source", value_enum, value_delimiter = ',')]
        sources: Vec<GameDiscoverySourceArg>,
        /// Internal: output streaming menu rows for the add flow
        #[arg(long, hide = true)]
        menu: bool,
        /// Disable the discovered-games cache for this run
        ///
        /// Cache file: `$XDG_CACHE_HOME/instant/game-discovery-cache.json`
        /// (usually `~/.cache/instant/game-discovery-cache.json`)
        #[arg(long)]
        no_cache: bool,
    },
    /// Sync game saves with restic repository
    #[command(alias = "update")]
    Sync {
        /// Game name to sync (optional, syncs all if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Force sync even if checkpoint matches
        #[arg(long)]
        force: bool,
    },
    /// Launch a game with automatic save sync
    Launch {
        /// Game name to launch (optional, will prompt if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
    },
    /// Execute a command with pre/post syncs (for Steam game prefixes)
    Exec {
        /// Command to execute (use with Steam %command% placeholder)
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<OsString>,
    },
    /// List all configured games
    List,
    /// Show detailed information about a game
    Info {
        /// Game name to show (optional, will prompt if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
    },
    /// Interactive game menu (launch or edit games)
    Menu {
        /// Game name (optional, will prompt if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Open the menu in a GUI terminal window
        #[arg(long = "gui")]
        gui: bool,
    },
    /// Remove a game from tracking
    Remove {
        /// Game name to remove (optional, will prompt if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Remove game without interactive confirmation
        #[arg(long)]
        force: bool,
    },
    /// Create a restic backup of game saves
    Backup {
        /// Game name to backup (optional, will prompt if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
    },
    /// Prune game snapshots using a retention policy
    Prune {
        /// Game name to prune (optional, prunes all if not specified)
        #[arg(long, add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Use legacy zero-change pruning behavior
        #[arg(long = "zero-changes")]
        zero_changes: bool,
    },
    /// Run restic commands with instant games repository configuration
    Restic {
        /// Restic command and arguments to execute
        args: Vec<String>,
    },
    /// Restore game saves from a backup snapshot
    Restore {
        /// Game name to restore (optional, will prompt if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Snapshot ID to restore from (optional, will prompt if not specified)
        snapshot_id: Option<String>,
        /// Force restore even if checkpoint matches
        #[arg(long)]
        force: bool,
    },
    /// List game backup snapshots
    #[command(alias = "history")]
    Snapshots {
        /// Game name to list snapshots for (optional, shows all game snapshots)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
    },
    /// Set up backup storage if needed, then configure games on this device
    Setup,
    /// Relocate a game's save path (updates config, does not move files)
    #[command(alias = "move")]
    Relocate {
        /// The new path for the game saves
        path: Option<String>,
        /// Game name (optional, will prompt if not specified)
        #[arg(long, short)]
        game: Option<String>,
    },
    /// Scan a Wine prefix for Ludusavi-compatible save games
    ScanWinePrefix {
        /// Wine prefix path (optional, prompts if omitted)
        prefix: Option<String>,
        /// Print results as a list instead of interactive menu
        #[arg(long)]
        list: bool,
    },
    /// Manage game dependencies
    Deps {
        /// Dependency subcommands
        #[command(subcommand)]
        command: DependencyCommands,
    },
    /// Debug command: Show snapshot tag information (for developers)
    #[cfg(debug_assertions)]
    Debug {
        /// Debug subcommands
        #[command(subcommand)]
        debug_command: DebugCommands,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GameDiscoverySourceArg {
    #[value(name = "switch", alias = "eden")]
    Switch,
    #[value(name = "ps2", alias = "pcsx2")]
    Ps2,
    #[value(name = "ps1", alias = "duckstation")]
    Ps1,
    #[value(name = "3ds", alias = "azahar")]
    ThreeDs,
    #[value(name = "epic")]
    Epic,
    #[value(name = "steam")]
    Steam,
    #[value(name = "faugus")]
    Faugus,
    #[value(name = "wine")]
    Wine,
}

/// Dependency management commands
#[derive(Subcommand, Debug, Clone)]
pub enum DependencyCommands {
    /// Add a dependency to a game and snapshot it into restic
    Add {
        /// Game name (optional, prompts if omitted)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Dependency identifier (optional, prompts if omitted)
        dependency_id: Option<String>,
        /// Path to dependency source file or directory (optional, prompts if omitted)
        path: Option<String>,
    },
    /// Install a dependency onto this device
    Install {
        /// Game name (optional, prompts if omitted)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Dependency identifier (optional, prompts if omitted)
        dependency_id: Option<String>,
        /// Destination path (optional, prompts if omitted)
        path: Option<String>,
    },
    /// Remove dependency installation record from this device
    Uninstall {
        /// Game name (optional, prompts if omitted)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
        /// Dependency identifier (optional, prompts if omitted)
        dependency_id: Option<String>,
    },
    /// List dependencies for a game
    List {
        /// Game name (optional, prompts if omitted)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
    },
}

/// Debug commands for developers
#[cfg(debug_assertions)]
#[derive(Subcommand, Debug, Clone)]
pub enum DebugCommands {
    /// Show detailed snapshot tag information
    Tags {
        /// Show tags for specific game (optional, shows all if not specified)
        #[arg(add = ArgValueCompleter::new(crate::completions::game_name_completion))]
        game_name: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum SaveGroupCommands {
    /// Declare a shared group and bind its local save path
    Create {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        path: Option<String>,
        /// Treat a missing save path as a single file
        #[arg(long, requires = "path")]
        file: bool,
    },
    /// Convert private games sharing exactly one path into a new group together
    #[command(
        after_help = "Example: ins game group migrate shared-card --name 'Shared PS2 card' 'Game A' 'Game B'\n\nInclude every game sharing this path. Files and private snapshots remain untouched; private checkpoints are not copied into group history. After conversion, use group reconcile shared-card --local to publish current saves, or group import to choose a private snapshot. Unfinished restores must be completed first."
    )]
    Migrate {
        id: String,
        #[arg(long)]
        name: String,
        /// All private games that currently share this card or save directory
        #[arg(required = true, num_args = 2..)]
        games: Vec<String>,
    },
    /// Bind an existing group to this device (resets local sync baseline)
    Bind {
        id: String,
        path: String,
        #[arg(long)]
        file: bool,
    },
    /// Assign a game to a group, retaining its previous snapshots
    Join {
        id: String,
        game: String,
        #[arg(long)]
        discard_private_binding: bool,
    },
    /// Remove an unreferenced group without deleting any snapshots
    Remove { id: String },
    /// List groups and their bindings on this device
    List,
    /// Choose a baseline or resolve conflicting versions
    Reconcile {
        id: String,
        #[arg(long, conflicts_with = "snapshot")]
        local: bool,
        #[arg(long)]
        snapshot: Option<String>,
    },
    /// Sync this group once
    Sync { id: String },
    /// Restore all saves in the group together
    Restore {
        id: String,
        snapshot: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// List this group's history
    Snapshots { id: String },
    /// Import an existing private game's snapshot as a new group baseline
    Import {
        id: String,
        game: String,
        snapshot: String,
    },
}
