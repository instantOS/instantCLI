use super::init::initialize_restic_repo;
use crate::dot::path_serde::TildePath;
use crate::fzf_wrapper::FzfWrapper;
use crate::game::config::InstantGameConfig;
use anyhow::{Context, Result};
use std::process::Command;

/// Manage restic repository initialization and configuration
pub struct RepositoryManager;

/// Options for initializing the game repository non-interactively
#[derive(Debug, Default)]
pub struct InitOptions {
    pub repo: Option<String>,
    pub password: Option<String>,
}

impl RepositoryManager {
    /// Initialize the game save backup system
    pub fn initialize_game_manager(debug: bool, options: InitOptions) -> Result<()> {
        println!("Initializing game save manager...");

        let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;

        if config.is_initialized() {
            match Self::handle_existing_configuration(&config, debug) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    // Check if this is a reconfiguration request
                    if e.to_string().contains("reconfiguration needed") {
                        println!("🔄 Starting repository reconfiguration...");
                        // Clear the current config to force new setup
                        config.repo =
                            crate::dot::path_serde::TildePath::new(std::path::PathBuf::new());
                        config.repo_password = "instantgamepassword".to_string();
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Self::setup_new_repository(&mut config, debug, &options)
    }

    /// Handle existing configuration by validating connection and offering recovery options
    fn handle_existing_configuration(config: &InstantGameConfig, debug: bool) -> Result<()> {
        println!("Game save manager appears to be already initialized.");
        println!(
            "Current repository: {}",
            config.repo.to_tilde_string().unwrap_or_else(|_| config
                .repo
                .as_path()
                .to_string_lossy()
                .to_string())
        );

        println!("🔍 Testing repository connection...");
        match Self::validate_repository_connection(&config.repo, &config.repo_password, debug) {
            Ok(()) => {
                println!("✅ Repository connection is working properly!");
                Ok(())
            }
            Err(e) => {
                println!("❌ Repository connection test failed: {e}");
                Self::handle_connection_failure(config, debug)
            }
        }
    }

    /// Handle repository connection failure with intelligent recovery options
    fn handle_connection_failure(config: &InstantGameConfig, debug: bool) -> Result<()> {
        let repo_str = config.repo.as_path().to_string_lossy();

        if Self::is_rclone_remote(&repo_str) {
            Self::handle_rclone_remote_failure(&repo_str, config, debug)
        } else {
            Self::handle_standard_repository_failure()
        }
    }

    /// Handle rclone remote connection failure
    fn handle_rclone_remote_failure(
        repo_str: &str,
        config: &InstantGameConfig,
        debug: bool,
    ) -> Result<()> {
        println!("🔍 Detected rclone remote configuration. Testing remote accessibility...");

        match Self::test_rclone_remote(repo_str, debug) {
            Ok(()) => Self::handle_accessible_remote_without_repo(config, debug),
            Err(remote_error) => Self::handle_inaccessible_remote(remote_error),
        }
    }

    /// Handle case where rclone remote is accessible but has no restic repository
    fn handle_accessible_remote_without_repo(
        config: &InstantGameConfig,
        debug: bool,
    ) -> Result<()> {
        println!("✅ Rclone remote is accessible!");
        println!("💡 The remote works, but no restic repository exists there yet.");

        // Use message dialog before the interactive prompt
        let message = "🎯 Repository Creation Options:\n\nYour rclone remote is working, but there's no restic repository there yet.";

        FzfWrapper::message(message)?;
        match FzfWrapper::confirm("Create restic repository in existing remote?")
            .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?
        {
            crate::fzf_wrapper::ConfirmResult::Yes => {
                Self::create_repository_in_existing_remote(config, debug)
            }
            crate::fzf_wrapper::ConfirmResult::No
            | crate::fzf_wrapper::ConfirmResult::Cancelled => Self::handle_declined_repo_creation(),
        }
    }

    /// Create restic repository in existing accessible remote
    fn create_repository_in_existing_remote(config: &InstantGameConfig, debug: bool) -> Result<()> {
        println!("🚀 Creating restic repository in existing remote...");
        if initialize_restic_repo(config.repo.as_path(), &config.repo_password, debug)? {
            println!("✅ Repository created successfully in existing remote!");
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Failed to create restic repository in remote"
            ))
        }
    }

    /// Handle when user declines to create repository in existing remote
    fn handle_declined_repo_creation() -> Result<()> {
        // Use message dialog before the interactive prompt
        let message = "📝 Repository Configuration:\n\nYou chose not to create a repository in the existing remote.\nWould you like to reconfigure the repository settings instead?";

        FzfWrapper::message(message)?;
        match FzfWrapper::confirm("Would you like to reconfigure the repository settings?")
            .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?
        {
            crate::fzf_wrapper::ConfirmResult::Yes => {
                println!("🔄 Proceeding with reconfiguration...");
                Err(anyhow::anyhow!("Repository reconfiguration needed"))
            }
            crate::fzf_wrapper::ConfirmResult::No
            | crate::fzf_wrapper::ConfirmResult::Cancelled => Err(anyhow::anyhow!(
                "Repository setup cancelled. Please reconfigure when ready."
            )),
        }
    }

    /// Handle inaccessible rclone remote
    fn handle_inaccessible_remote(remote_error: anyhow::Error) -> Result<()> {
        // Use message dialog before the interactive prompt
        let message = format!(
            "❌ Rclone Remote Issue:\n\nRclone remote test failed: {remote_error}\n💡 The remote configuration may be incorrect or inaccessible.\n\nWould you like to reconfigure the repository settings?"
        );

        FzfWrapper::message(&message)?;
        match FzfWrapper::confirm("Would you like to reconfigure the repository settings?")
            .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?
        {
            crate::fzf_wrapper::ConfirmResult::Yes => {
                println!("🔄 Proceeding with reconfiguration...");
                Err(anyhow::anyhow!("Repository reconfiguration needed"))
            }
            crate::fzf_wrapper::ConfirmResult::No
            | crate::fzf_wrapper::ConfirmResult::Cancelled => Err(anyhow::anyhow!(
                "Remote is not accessible. Please check your rclone configuration and network connection."
            )),
        }
    }

    /// Handle standard (non-rclone) repository failure
    fn handle_standard_repository_failure() -> Result<()> {
        // Use message dialog before the interactive prompt
        let message = "❌ Repository Connection Failed:\n\nThe repository connection failed.\nWould you like to reconfigure the repository settings?";

        FzfWrapper::message(message)?;
        match FzfWrapper::confirm("Would you like to reconfigure the repository settings?")
            .map_err(|e| anyhow::anyhow!("Failed to get user input: {}", e))?
        {
            crate::fzf_wrapper::ConfirmResult::Yes => {
                println!("🔄 Proceeding with reconfiguration...");
                Err(anyhow::anyhow!("Repository reconfiguration needed"))
            }
            crate::fzf_wrapper::ConfirmResult::No
            | crate::fzf_wrapper::ConfirmResult::Cancelled => Err(anyhow::anyhow!(
                "Repository connection failed. Please check your configuration."
            )),
        }
    }

    /// Setup a new repository configuration from scratch
    fn setup_new_repository(
        config: &mut InstantGameConfig,
        debug: bool,
        options: &InitOptions,
    ) -> Result<()> {
        // Prompt for restic repository using fzf unless provided
        let repo = if let Some(repo_str) = &options.repo {
            let tilde_path = TildePath::from_str(repo_str)
                .map_err(|e| anyhow::anyhow!("Invalid repository path: {}", e))?;

            if tilde_path.as_path().is_absolute() && !tilde_path.as_path().exists() {
                std::fs::create_dir_all(tilde_path.as_path())
                    .context("Failed to create repository directory")?;
            }

            tilde_path
        } else {
            Self::get_repository_path()?
        };

        let password = options
            .password
            .clone()
            .unwrap_or_else(|| "instantgamepassword".to_string());

        // Update config
        config.repo = repo.clone();
        config.repo_password = password.clone();

        // Initialize the repository
        if initialize_restic_repo(repo.as_path(), &password, debug)? {
            config.save()?;
            println!("✅ Game save manager initialized successfully!");
            println!(
                "Repository: {}",
                repo.to_tilde_string()
                    .unwrap_or_else(|_| repo.as_path().to_string_lossy().to_string())
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to connect to restic repository"))
        }
    }

    /// Validate that the repository connection actually works
    fn validate_repository_connection(repo: &TildePath, password: &str, debug: bool) -> Result<()> {
        if debug {
            println!(
                "Testing repository connection: {}",
                repo.to_tilde_string()
                    .unwrap_or_else(|_| repo.as_path().to_string_lossy().to_string())
            );
        }

        let restic = crate::restic::ResticWrapper::new(
            repo.as_path().to_string_lossy().to_string(),
            password.to_string(),
        );

        // Try to check if repository exists - this will test the actual connection
        match restic.repository_exists() {
            Ok(exists) => {
                if exists {
                    if debug {
                        println!("✓ Repository exists and is accessible");
                    }

                    // Additional test: try to list snapshots to ensure the repository is fully functional
                    match restic.list_snapshots_filtered(None) {
                        Ok(_) => {
                            if debug {
                                println!("✓ Repository operations working correctly");
                            }
                            Ok(())
                        }
                        Err(e) => Err(anyhow::anyhow!(
                            "Repository exists but operations failed: {}",
                            e
                        )),
                    }
                } else {
                    Err(anyhow::anyhow!("Repository does not exist"))
                }
            }
            Err(e) => Err(anyhow::anyhow!("Failed to connect to repository: {}", e)),
        }
    }

    /// Test if an rclone remote is accessible
    fn test_rclone_remote(repo_str: &str, debug: bool) -> Result<()> {
        if debug {
            println!("Testing rclone remote accessibility: {repo_str}");
        }

        // Extract the remote path for rclone lsd command
        // rclone:remote:path -> remote:path
        let rclone_path = repo_str
            .strip_prefix("rclone:")
            .ok_or_else(|| anyhow::anyhow!("Invalid rclone remote format: {}", repo_str))?;

        let output = Command::new("rclone")
            .args(["lsd", rclone_path])
            .output()
            .context(
                "Failed to execute rclone command. Make sure rclone is installed and in PATH.",
            )?;

        if output.status.success() {
            if debug {
                println!("✓ Rclone remote is accessible");
            }
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!(
                "Rclone remote test failed: {}",
                stderr.trim()
            ))
        }
    }

    /// Check if a repository path looks like an rclone remote
    fn is_rclone_remote(repo_str: &str) -> bool {
        repo_str.starts_with("rclone:")
    }

    /// Get repository path from user input or use default
    fn get_repository_path() -> Result<TildePath> {
        // Show helpful information about repository formats using message dialog
        let message = "📁 Repository Options:\n\n• Local path: /path/to/repo or ~/games/repo\n• Rclone remote: rclone:remote:name\n• SFTP: sftp:user@host:/path\n• S3: s3:bucketname\n• Other: See restic documentation for supported backends\n\nEnter your repository path or leave empty for default local repository.";

        // Show message then prompt for input
        FzfWrapper::message(message)?;

        let repo_input = FzfWrapper::input("Enter restic repository path or URL")
            .map_err(|e| anyhow::anyhow!("Failed to get repository input: {}", e))?
            .trim()
            .to_string();

        // Use default if empty
        if repo_input.is_empty() {
            let default_path = dirs::data_dir()
                .unwrap_or_else(|| {
                    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"));
                    home.join(".local/share")
                })
                .join("instantos")
                .join("games")
                .join("repo");
            println!(
                "Using default local repository: {}",
                default_path.to_string_lossy()
            );
            Ok(TildePath::new(default_path))
        } else {
            // Use TildePath to handle tilde expansion automatically
            let path = TildePath::from_str(&repo_input)?;

            // Provide guidance for rclone remotes (after interaction, so println is fine)
            if repo_input.starts_with("rclone:") {
                println!("🔧 Configuring rclone remote: {repo_input}");
                println!("💡 Make sure your rclone is configured and the remote exists");
                println!("💡 Test with: rclone lsd {repo_input}");
            }

            Ok(path)
        }
    }
}
