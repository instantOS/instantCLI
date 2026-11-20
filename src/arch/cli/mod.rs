mod commands;
mod utils;

use clap::Subcommand;

pub use commands::{handle_arch_command, print_config_guide};

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommands {
    /// Print a starter TOML config with hardware detected on this machine
    Example,
    /// List config fields or explain one field's values and requirements
    Explain {
        /// TOML field name, such as PartitioningMethod
        field: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DualbootCommands {
    /// Show information about existing operating systems and partitions
    Info,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ArchCommands {
    /// Start the Arch Linux installation wizard
    Install,
    /// Internal entry point for the private Arch bootstrap namespace
    #[command(hide = true)]
    BootstrapEnter {
        #[arg(long)]
        root: std::path::PathBuf,
    },
    /// List all available questions
    List,
    /// Get a starter TOML config or look up its fields
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Validate a config and summarize its install plan without executing it
    Check {
        /// Path to the questions TOML file
        #[arg(short = 'f', long)]
        questions_file: std::path::PathBuf,
        /// Also require the planned disk and partitions to exist as block devices
        #[arg(long)]
        target: bool,
    },
    /// Ask a specific question
    Ask {
        /// The ID of the question to ask
        #[arg(value_enum)]
        id: Option<crate::arch::engine::StepId>,
        /// Optional path to save the configuration TOML file
        #[arg(short = 'o', long)]
        output_config: Option<std::path::PathBuf>,
    },
    /// Execute installation steps based on a questions file
    Exec {
        /// The step to execute (optional, defaults to all steps)
        #[arg(value_enum)]
        step: Option<String>,
        /// Path to the questions TOML file. Defaults to this run's own
        /// configuration file for the current host.
        // `-f` is the flag the shell bootstrap and the e2e suite pass; it is
        // pinned here because deriving it from the long name would give `-q`.
        #[arg(short = 'f', long = "questions-file")]
        questions_file: Option<std::path::PathBuf>,
        /// Run in dry-run mode (no changes will be made)
        #[arg(long)]
        dry_run: bool,
    },
    /// Show installation finished menu
    Finished,
    /// Setup instantOS on an existing Arch Linux installation
    Setup {
        /// Optional username to setup dotfiles for
        #[arg(short, long)]
        user: Option<String>,
        /// Run in dry-run mode
        #[arg(long)]
        dry_run: bool,
    },
    /// Review and optionally upload a privacy-filtered support report to snips.sh
    UploadLogs {
        /// Upload this exact file verbatim instead of building a filtered report
        #[arg(short, long)]
        path: Option<std::path::PathBuf>,
    },
    /// Show system information in a pretty format
    Info,
    /// Dual boot detection and setup
    Dualboot {
        #[command(subcommand)]
        command: DualbootCommands,
    },
}
