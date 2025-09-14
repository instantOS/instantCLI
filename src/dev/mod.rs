use anyhow::Result;
use clap::Subcommand;

mod clone;
mod fuzzy;
mod github;

pub use clone::*;
pub use fuzzy::*;
pub use github::*;

#[derive(Subcommand, Debug, Clone)]
pub enum DevCommands {
    Clone,
}

pub async fn handle_dev_command(command: DevCommands, debug: bool) -> Result<()> {
    match command {
        DevCommands::Clone => handle_clone(debug).await,
    }
}

async fn handle_clone(debug: bool) -> Result<()> {
    if debug {
        eprintln!("🔍 Fetching instantOS repositories...");
    }

    let pb = crate::common::create_spinner("Fetching repositories from GitHub...".to_string());

    let repos = fetch_instantos_repos().await
        .map_err(|e| anyhow::anyhow!("Failed to fetch repositories: {}", e))?;

    pb.finish_with_message(format!("Found {} repositories", repos.len()));

    if debug {
        eprintln!("📋 Available repositories:");
        for repo in &repos {
            eprintln!("  - {} ({})", repo.name, repo.full_name);
        }
    }

    let selected_repo = select_repository(repos)
        .map_err(|e| anyhow::anyhow!("Failed to select repository: {}", e))?;

    if debug {
        eprintln!("🎯 Selected repository: {}", selected_repo.name);
    }

    let workspace_dir = ensure_workspace_dir()?;

    let target_dir = workspace_dir.join(&selected_repo.name);

    clone_repository(&selected_repo, &target_dir, debug)?;

    Ok(())
}