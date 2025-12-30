use crate::ui::nerd_font::NerdFont;
use anyhow::Result;
use clap::Subcommand;

mod clone;
mod fuzzy;
mod github;
mod install;
mod package;
mod setup;

pub use clone::{clone_repository, ensure_workspace_dir, CloneError};
pub use fuzzy::{select_package, select_repository, FzfError, GitHubRepoSelectItem, PackageSelectItem};
pub use github::{fetch_instantos_repos, GitHubError, GitHubErrorKind, GitHubRepo};
pub use install::{build_and_install_package, handle_install, PackageRepo};

#[derive(Subcommand, Debug, Clone)]
pub enum DevCommands {
    Clone,
    Install,
    /// Setup development environment (Arch live ISO)
    Setup,
}

pub async fn handle_dev_command(command: DevCommands, debug: bool) -> Result<()> {
    match command {
        DevCommands::Clone => handle_clone(debug).await,
        DevCommands::Install => handle_install(debug).await,
        DevCommands::Setup => setup::handle_setup(debug).await,
    }
}

async fn handle_clone(debug: bool) -> Result<()> {
    if debug {
        eprintln!(
            "{} Fetching instantOS repositories...",
            char::from(NerdFont::Search)
        );
    }

    let pb =
        crate::common::progress::create_spinner("Fetching repositories from GitHub...".to_string());

    let repos = fetch_instantos_repos()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch repositories: {}", e))?;

    crate::common::progress::finish_spinner_with_success(
        pb,
        format!("Found {} repositories", repos.len()),
    );

    if debug {
        eprintln!("{} Available repositories:", char::from(NerdFont::List));
        for repo in &repos {
            eprintln!("  - {} ({})", repo.name, repo.full_name);
        }
    }

    let selected_repo = select_repository(repos)
        .map_err(|e| anyhow::anyhow!("Failed to select repository: {}", e))?;

    if debug {
        eprintln!(
            "{} Selected repository: {}",
            char::from(NerdFont::SourceBranch),
            selected_repo.name
        );
    }

    let workspace_dir = ensure_workspace_dir()?;

    let target_dir = workspace_dir.join(&selected_repo.name);

    clone_repository(&selected_repo, &target_dir, debug)?;

    Ok(())
}
