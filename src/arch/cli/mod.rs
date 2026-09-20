mod commands;
mod utils;

use clap::Subcommand;

pub use commands::handle_arch_command;

pub(crate) const DEFAULT_QUESTIONS_FILE: &str = "/etc/instant/questions.toml";

#[derive(Subcommand, Debug, Clone)]
pub enum DualbootCommands {
    /// Show information about existing operating systems and partitions
    Info,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ArchCommands {
    /// Start the Arch Linux installation wizard
    Install,
    /// List all available questions
    List,
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
        /// Path to the questions TOML file
        #[arg(short = 'f', long = "questions-file", default_value = DEFAULT_QUESTIONS_FILE)]
        questions_file: std::path::PathBuf,
        /// Run in dry-run mode (no changes will be made)
        #[arg(long)]
        dry_run: bool,
        /// Skip dependency-provenance validation for hand-edited configuration
        /// files. Answers are still validated; missing or stale provenance
        /// records are recomputed instead of rejected.
        #[arg(long)]
        trust_config: bool,
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
