use anyhow::Result;
use clap::Subcommand;

use crate::restic::logging::ResticCommandLogger;
use crate::ui::{emit, Level};
use crate::ui::nerd_font::NerdFont;

#[derive(Subcommand, Debug, Clone)]
pub enum DebugCommands {
    /// View restic command logs
    ResticLogs {
        /// Number of recent logs to show (default: 10)
        #[arg(short, long)]
        limit: Option<usize>,
        /// Clear all logs
        #[arg(long)]
        clear: bool,
    },
}

pub fn handle_debug_command(command: DebugCommands) -> Result<()> {
    match command {
        DebugCommands::ResticLogs { limit, clear } => {
            let logger = ResticCommandLogger::new()?;

            if clear {
                logger.clear_logs()?;
                emit(
                    Level::Success,
                    "restic.logs.cleared",
                    &format!(
                        "{} Cleared all restic command logs.",
                        char::from(NerdFont::Trash)
                    ),
                    None,
                );
            } else {
                logger.print_recent_logs(limit)?;
            }
        }
    }

    Ok(())
}