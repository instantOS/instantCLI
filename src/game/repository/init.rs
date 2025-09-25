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

    let restic = ResticWrapper::with_debug(repo.to_string_lossy().to_string(), password.to_string(), debug);

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
    if repo.is_absolute() && !repo.exists() {
        eprintln!("Repository path does not exist.");

        match FzfWrapper::confirm("Would you like to create it?")
            .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?
        {
            ConfirmResult::Yes => {
                // Create parent directories
                std::fs::create_dir_all(repo).context("Failed to create repository directory")?;
                println!("✓ Created repository directory");
            }
            ConfirmResult::No | ConfirmResult::Cancelled => {
                println!("Repository initialization cancelled.");
                return Ok(false);
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
