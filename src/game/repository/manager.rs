use std::io::IsTerminal;

use anyhow::{Context, Result, bail};

use crate::common::{TildePath, paths};
use crate::game::config::{InstantGameConfig, games_config_path};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::{DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::restic::error::ResticError;
use crate::ui::nerd_font::NerdFont;

use super::init::{RepositoryIntent, prepare_repository};
use super::rclone;

pub struct GameRepositoryManager;

#[derive(Default)]
pub struct InitOptions {
    pub repo: Option<String>,
    pub password: Option<String>,
    pub existing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitOutcome {
    Ready,
    Cancelled,
}

impl GameRepositoryManager {
    pub fn initialize_game_manager(_debug: bool, options: InitOptions) -> Result<InitOutcome> {
        let mut config = InstantGameConfig::load().context("Failed to load game configuration")?;
        if let Some(repo) = options.repo {
            let repository = normalize_repository(&repo)?;
            let password = options.password.unwrap_or_else(default_password);
            validate_password(&password)?;
            let intent = if options.existing {
                RepositoryIntent::Connect
            } else {
                RepositoryIntent::Create
            };
            println!("Checking backup storage: {repository}");
            prepare_repository(&repository, &password, intent)?;
            save_repository(&mut config, &repository, password)?;
            print_completion(&repository)?;
            return Ok(InitOutcome::Ready);
        }

        if !std::io::stdin().is_terminal() {
            bail!(
                "Interactive setup needs a terminal. Use `ins game init --repo PATH` to create backups, or add --existing to connect. Use --password only for a custom repository password."
            );
        }
        run_wizard(&mut config, options)
    }
}

fn default_password() -> String {
    InstantGameConfig::default().repo_password
}

#[derive(Clone)]
struct Choice<T> {
    label: &'static str,
    description: &'static str,
    value: T,
}

impl<T: Clone> FzfSelectable for Choice<T> {
    fn fzf_display_text(&self) -> String {
        self.label.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Text(self.description.to_string())
    }
}

fn choose<T: Clone>(title: &str, subtitle: &str, choices: Vec<Choice<T>>) -> Result<Option<T>> {
    match FzfWrapper::builder()
        .header(
            HeaderBuilder::new(NerdFont::BackupRestore, title)
                .subtitle(subtitle)
                .build(),
        )
        .items(choices)
        .select_one()?
    {
        DialogOutcome::Submitted(choice) => Ok(Some(choice.value)),
        DialogOutcome::Cancelled => Ok(None),
    }
}

fn choose_intent() -> Result<Option<RepositoryIntent>> {
    choose(
        "Set up game backups",
        "Esc cancels. No repository settings are saved until storage is verified.",
        vec![
            Choice {
                label: "Create new backups",
                description: "Start a new backup repository. Choose a dedicated folder; an existing repository will never be overwritten.",
                value: RepositoryIntent::Create,
            },
            Choice {
                label: "Connect existing backups",
                description: "Use backups from another device. Select the same storage and folder. Games with backups can then be configured on this device.",
                value: RepositoryIntent::Connect,
            },
        ],
    )
}

#[derive(Clone, Copy)]
enum Storage {
    Cloud,
    Local,
    Advanced,
}

fn choose_storage() -> Result<Option<String>> {
    loop {
        let Some(storage) = choose(
            "Where should game backups live?",
            "Cloud storage can be shared across devices. A local folder alone does not sync between machines.",
            vec![
                Choice {
                    label: "Cloud storage (rclone)",
                    description: "Choose an existing rclone remote or configure your cloud account. Then select a folder for game backups.",
                    value: Storage::Cloud,
                },
                Choice {
                    label: "Local folder",
                    description: "Store backups on this computer, an external drive, or a mounted network share. This is separate from your game's save folder.",
                    value: Storage::Local,
                },
                Choice {
                    label: "Advanced: repository URL",
                    description: "Enter a restic backend URL, such as sftp:user@host:/backups/games, or rclone:remote:folder. Backend tools and credentials must already be configured.",
                    value: Storage::Advanced,
                },
            ],
        )?
        else {
            return Ok(None);
        };
        let selected = match storage {
            Storage::Cloud => rclone::choose_repository()?,
            Storage::Local => prompt_local_repository()?,
            Storage::Advanced => prompt_advanced_repository()?,
        };
        if selected.is_some() {
            return Ok(selected);
        }
        // Cancelling a storage-specific screen returns to storage selection.
    }
}

fn prompt_local_repository() -> Result<Option<String>> {
    let default = paths::default_games_repo_path()
        .to_string_lossy()
        .to_string();
    loop {
        match FzfWrapper::input(&format!(
            "Backup folder (not the game's saves). Leave empty for {default}"
        ))? {
            DialogOutcome::Cancelled => return Ok(None),
            DialogOutcome::Submitted(input) => {
                let input = if input.trim().is_empty() {
                    &default
                } else {
                    input.trim()
                };
                let path = TildePath::from_str(input);
                if !path.as_path().is_absolute() {
                    FzfWrapper::message("Use an absolute folder path or a path starting with ~/.")?;
                    continue;
                }
                return Ok(Some(path.as_path().to_string_lossy().to_string()));
            }
        }
    }
}

fn prompt_advanced_repository() -> Result<Option<String>> {
    loop {
        match FzfWrapper::input(
            "Restic repository URL or absolute path (rclone format: rclone:remote:folder)",
        )? {
            DialogOutcome::Cancelled => return Ok(None),
            DialogOutcome::Submitted(input) => match normalize_repository(&input) {
                Ok(repository) => return Ok(Some(repository)),
                Err(error) => FzfWrapper::message(&error.to_string())?,
            },
        }
    }
}

fn normalize_repository(input: &str) -> Result<String> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_control) {
        bail!("Enter a repository URL or absolute folder path.");
    }
    let path = TildePath::from_str(input);
    if !path.as_path().is_absolute() && !input.contains(':') {
        bail!("Use an absolute folder path, ~/path, or a restic backend URL.");
    }
    Ok(path.as_path().to_string_lossy().to_string())
}

#[derive(Clone, Copy)]
enum ReviewAction {
    Proceed,
    Storage,
    Password,
    Intent,
}

fn review(
    repository: &str,
    password: &str,
    intent: RepositoryIntent,
    error: Option<&str>,
) -> Result<Option<ReviewAction>> {
    let action = match intent {
        RepositoryIntent::Create => "Create new backups",
        RepositoryIntent::Connect => "Connect existing backups",
    };
    let password_info = if password == default_password() {
        "Built-in password: easy to use across devices, NOT private encryption. Access protection comes from your storage account."
    } else {
        "Custom password: saved locally in plaintext for automatic sync. Keep a separate copy for another device or recovery."
    };
    let mut summary = format!(
        "Action: {action}\nStorage: {repository}\n{password_info}\nExisting game entries and save paths are kept. No game saves are restored here."
    );
    if let Some(error) = error {
        summary.push_str(&format!("\n\nStorage check failed: {error}\nNo repository settings were saved. Edit a setting below, or retry."));
    }
    choose(
        "Review backup setup",
        &summary,
        vec![
            Choice {
                label: "Continue: verify and save",
                description: "Perform the selected action, then save repository settings. Creating backups writes a new restic repository at this location; connecting only verifies access.",
                value: ReviewAction::Proceed,
            },
            Choice {
                label: "Choose a different storage location",
                description: "Select another remote, folder, or repository URL.",
                value: ReviewAction::Storage,
            },
            Choice {
                label: "Advanced: repository password",
                description: "Use the built-in password for easy multi-device setup, or set/enter a custom password. Changing this setting does NOT change a password on an existing repository.",
                value: ReviewAction::Password,
            },
            Choice {
                label: "Change: new or existing backups",
                description: "Switch between creating a repository and connecting to one that already exists.",
                value: ReviewAction::Intent,
            },
        ],
    )
}

fn prompt_password(intent: RepositoryIntent) -> Result<Option<String>> {
    let Some(custom) = choose(
        "Repository password",
        "The built-in password needs no transfer between devices, but provides no private encryption.",
        vec![
            Choice {
                label: "Use built-in password (default)",
                description: "Use the same built-in password as other instantCLI installations. Protect your backups using your storage account permissions.",
                value: false,
            },
            Choice {
                label: "Use a custom password",
                description: "Saved in plaintext in the local games.toml for unattended sync. You must keep a separate copy and use the same password on other devices. Lost passwords cannot be reset to recover backups.",
                value: true,
            },
        ],
    )?
    else {
        return Ok(None);
    };
    if !custom {
        return Ok(Some(default_password()));
    }
    loop {
        let builder = FzfWrapper::builder()
            .prompt(match intent {
                RepositoryIntent::Create => "New repository password",
                RepositoryIntent::Connect => "Password for existing backups",
            })
            .password();
        let outcome = match intent {
            RepositoryIntent::Create => builder.with_confirmation().password_dialog()?,
            RepositoryIntent::Connect => builder.password_dialog()?,
        };
        match outcome {
            DialogOutcome::Cancelled => return Ok(None),
            DialogOutcome::Submitted(password) => match validate_password(&password) {
                Ok(()) => return Ok(Some(password)),
                Err(error) => FzfWrapper::message(&error.to_string())?,
            },
        }
    }
}

fn validate_password(password: &str) -> Result<()> {
    if password.is_empty() || password.contains(['\0', '\n', '\r']) {
        bail!("The repository password cannot be empty or contain NUL/newline characters.");
    }
    Ok(())
}

fn run_wizard(config: &mut InstantGameConfig, options: InitOptions) -> Result<InitOutcome> {
    let (mut intent, mut repository, mut password) = if config.is_initialized() {
        (
            RepositoryIntent::Connect,
            config.repo.as_path().to_string_lossy().to_string(),
            config.repo_password.clone(),
        )
    } else {
        let intent = if options.existing {
            RepositoryIntent::Connect
        } else {
            let Some(intent) = choose_intent()? else {
                return Ok(InitOutcome::Cancelled);
            };
            intent
        };
        let Some(repository) = choose_storage()? else {
            return Ok(InitOutcome::Cancelled);
        };
        (intent, repository, default_password())
    };
    if let Some(supplied) = options.password {
        validate_password(&supplied)?;
        password = supplied;
    }
    let mut error = None;
    loop {
        match review(&repository, &password, intent, error.as_deref())? {
            None => {
                println!("Backup setup cancelled. Repository settings unchanged.");
                return Ok(InitOutcome::Cancelled);
            }
            Some(ReviewAction::Storage) => {
                if let Some(selected) = choose_storage()? {
                    repository = selected;
                    error = None;
                }
            }
            Some(ReviewAction::Password) => {
                if let Some(selected) = prompt_password(intent)? {
                    password = selected;
                    error = None;
                }
            }
            Some(ReviewAction::Intent) => {
                if let Some(selected) = choose_intent()? {
                    intent = selected;
                    error = None;
                }
            }
            Some(ReviewAction::Proceed) => {
                println!("Checking backup storage: {repository}");
                match prepare_repository(&repository, &password, intent) {
                    Ok(()) => {
                        save_repository(config, &repository, password)?;
                        print_completion(&repository)?;
                        return Ok(InitOutcome::Ready);
                    }
                    Err(failure) => error = Some(connection_guidance(&failure)),
                }
            }
        }
    }
}

fn connection_guidance(error: &anyhow::Error) -> String {
    match error.downcast_ref::<ResticError>() {
        Some(ResticError::InvalidPassword) => "The repository password is incorrect. Choose Advanced: repository password and enter the password used when these backups were created.".to_string(),
        Some(ResticError::RepositoryLocked) => "The repository is locked. Wait for other backup operations to finish, then retry.".to_string(),
        _ => format!("{error:#}\nCheck your network, storage credentials and folder. For rclone, use the storage picker to test or reconfigure the remote."),
    }
}

fn save_repository(
    config: &mut InstantGameConfig,
    repository: &str,
    password: String,
) -> Result<()> {
    if config.repo.as_path().to_string_lossy() != repository {
        let mut installations = crate::game::config::InstallationsConfig::load()?;
        if installations
            .installations
            .iter()
            .any(|installation| installation.pending_restore.is_some())
        {
            bail!(
                "A game restore is incomplete. Finish `ins game setup` with the current repository before changing storage."
            );
        }
        // Bind legacy installations to the old location BEFORE saving the new
        // one. If either write fails, sync must not silently use the new storage.
        for installation in &mut installations.installations {
            if installation.sync_repository.is_none() {
                installation.sync_repository =
                    Some(config.repo.as_path().to_string_lossy().to_string());
            }
        }
        installations
            .save()
            .context("Could not preserve game repository associations")?;
    }
    let mut updated = config.clone();
    updated.repo = TildePath::from_str(repository);
    updated.repo_password = password;
    updated.save().context("Storage is ready, but saving local settings failed. Re-run setup with 'Connect existing backups' to retry; do not create another repository.")?;
    *config = updated;
    Ok(())
}

fn print_completion(repository: &str) -> Result<()> {
    println!("Game backup storage is ready: {repository}");
    println!(
        "Settings and repository password are saved locally in {} (owner-only access).",
        games_config_path()?.display()
    );
    println!(
        "On another device: run `ins game setup`, choose Connect existing backups, and select the same storage folder. Custom passwords must also match."
    );
    println!(
        "Next: `ins game add` to track a game, or `ins game setup` to configure games found in backups."
    );
    println!(
        "Use `ins game launch` for automatic sync before/after play, or `ins game sync` for manual sync. Setup alone does not enable background sync."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::{MockQueue, scripted_responses_remaining};

    #[test]
    fn cancelling_local_input_does_not_select_default() {
        let _guard = MockQueue::new().input_cancelled().guard();
        assert!(prompt_local_repository().unwrap().is_none());
    }

    #[test]
    fn blank_local_input_deliberately_selects_default() {
        let _guard = MockQueue::new().input_string("").guard();
        assert_eq!(
            prompt_local_repository().unwrap(),
            Some(
                paths::default_games_repo_path()
                    .to_string_lossy()
                    .to_string()
            )
        );
    }

    #[test]
    fn default_password_requires_no_secret_input() {
        let _guard = MockQueue::new().select_index(0).guard();
        assert_eq!(
            prompt_password(RepositoryIntent::Create).unwrap(),
            Some(default_password())
        );
        assert_eq!(scripted_responses_remaining(), 0);
    }

    #[test]
    fn custom_password_preserves_spaces_and_cancels_cleanly() {
        {
            let _guard = MockQueue::new()
                .select_index(1)
                .password(" secret ")
                .guard();
            assert_eq!(
                prompt_password(RepositoryIntent::Connect).unwrap(),
                Some(" secret ".to_string())
            );
        }
        let _guard = MockQueue::new()
            .select_index(1)
            .password_cancelled()
            .guard();
        assert!(prompt_password(RepositoryIntent::Create).unwrap().is_none());
    }

    #[test]
    fn cancelling_review_keeps_configuration() {
        let mut config = InstantGameConfig::default();
        config.repo = TildePath::from_str("/old/repository");
        config.repo_password = "original".to_string();
        let _guard = MockQueue::new().cancel_selection().guard();
        assert_eq!(
            run_wizard(&mut config, InitOptions::default()).unwrap(),
            InitOutcome::Cancelled
        );
        assert_eq!(config.repo.display_string(), "/old/repository");
        assert_eq!(config.repo_password, "original");
    }

    #[test]
    fn cancelling_fresh_setup_before_storage_has_no_effect() {
        let mut config = InstantGameConfig::default();
        let _guard = MockQueue::new().select_index(0).cancel_selection().guard();
        assert_eq!(
            run_wizard(&mut config, InitOptions::default()).unwrap(),
            InitOutcome::Cancelled
        );
        assert!(!config.is_initialized());
    }

    #[test]
    fn rejects_empty_repository_and_relative_paths() {
        for input in ["", " ", "folder", "./folder", "remote:\nfolder"] {
            assert!(normalize_repository(input).is_err(), "{input:?}");
        }
        assert_eq!(
            normalize_repository("rclone:drive:game saves").unwrap(),
            "rclone:drive:game saves"
        );
    }

    #[test]
    fn password_errors_have_specific_recovery() {
        let error = anyhow::Error::new(ResticError::InvalidPassword);
        let message = connection_guidance(&error);
        assert!(message.contains("Advanced: repository password"));
        assert!(!message.contains("no restic repository"));
    }
}
