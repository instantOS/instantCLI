use crate::fzf_wrapper::ConfirmResult;
use crate::fzf_wrapper::FzfWrapper;
use crate::restic::ResticWrapper;
use anyhow::{Context, Result};
use colored::*;
use std::path::Path;

/// Initialize a restic repository for game save backups
pub fn initialize_restic_repo(repo: &Path, password: &str, debug: bool) -> Result<bool> {
    if debug {
        println!(
            "Initializing restic repository: {}",
            repo.to_string_lossy().blue()
        );
    }

    let restic = ResticWrapper::new(repo.to_string_lossy().to_string(), password.to_string());

    // Check if repository already exists
    match restic.repository_exists() {
        Ok(exists) => {
            if exists {
                if debug {
                    println!(
                        "{}",
                        "✓ Repository already exists and is accessible".green()
                    );
                }
                return Ok(true);
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to check repository existence: {}",
                e
            ));
        }
    }

    // Repository doesn't exist, initialize it
    if repo.is_absolute() {
        if !repo.exists() {
            FzfWrapper::message("Repository path does not exist.")
                .context("Failed to show path error message")?;

            match FzfWrapper::confirm("Would you like to create it?")
                .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?
            {
                ConfirmResult::Yes => {
                    // Create parent directories
                    std::fs::create_dir_all(repo)
                        .context("Failed to create repository directory")?;
                    FzfWrapper::message("✓ Created repository directory")
                        .context("Failed to show directory created message")?;
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    FzfWrapper::message("Repository initialization cancelled.")
                        .context("Failed to show cancellation message")?;
                    return Ok(false);
                }
            }
        }
    }

    // Initialize the repository
    if debug {
        println!("{}", "Creating new restic repository...".blue());
    }

    match restic.init_repository() {
        Ok(()) => {
            if debug {
                println!("{}", "✓ Repository initialized successfully".green());
            }
            Ok(true)
        }
        Err(e) => Err(anyhow::anyhow!("Failed to initialize repository: {}", e)),
    }
}
