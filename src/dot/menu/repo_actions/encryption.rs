use anyhow::Result;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Debug, Clone)]
enum RepoEncryptionAction {
    ShowStatus,
    AuthorizeLocalKey,
    AuthorizeRemoteKey,
    RotateKeys,
    Back,
}

#[derive(Clone)]
struct RepoEncryptionItem {
    action: RepoEncryptionAction,
    display: String,
    preview: String,
}

impl FzfSelectable for RepoEncryptionItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        match self.action {
            RepoEncryptionAction::ShowStatus => "show_status".to_string(),
            RepoEncryptionAction::AuthorizeLocalKey => "authorize_local".to_string(),
            RepoEncryptionAction::AuthorizeRemoteKey => "authorize_remote".to_string(),
            RepoEncryptionAction::RotateKeys => "rotate_keys".to_string(),
            RepoEncryptionAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
        crate::menu::protocol::FzfPreview::Text(self.preview.clone())
    }
}

fn build_status_message(repo_name: &str, config: &DotfileConfig) -> String {
    let Ok(dotfile_repo) = crate::dot::dotfilerepo::DotfileRepo::new(config, repo_name.to_string())
    else {
        return format!("Repository '{}' not found.", repo_name);
    };
    let Ok(repo_path) = dotfile_repo.local_path(config) else {
        return format!("Repository path not found for '{}'.", repo_name);
    };
    let Ok(meta) = crate::dot::meta::read_meta(&repo_path) else {
        return format!("Could not read metadata for '{}'.", repo_name);
    };
    let local_keys = crate::dot::operations::key::get_local_public_keys().unwrap_or_default();

    if meta.age_recipients.is_empty() {
        return format!(
            "Repository: {}\n\nEncryption is not configured.\nNo age recipients have been authorized yet.",
            repo_name
        );
    }

    let local_authorized = meta.age_recipients.iter().any(|r| local_keys.contains(r));

    let mut encrypted_files = 0;
    for dir in &dotfile_repo.dotfile_dirs {
        if dir.path.is_dir() {
            encrypted_files += count_age_files(&dir.path);
        }
    }

    let auth_icon = if local_authorized { "✓" } else { "✗" };
    let auth_note = if local_authorized {
        String::new()
    } else {
        "\n\nYour key is NOT authorized.\nYou cannot decrypt files for this repo.\nRun 'Authorize Local Key' to fix this.".to_string()
    };

    format!(
        "Repository: {}\n\nRecipients: {}\nEncrypted files: {}\nLocal key: {} {}\n{}",
        repo_name,
        meta.age_recipients.len(),
        encrypted_files,
        auth_icon,
        if local_authorized {
            "Authorized"
        } else {
            "Unauthorized"
        },
        auth_note,
    )
}

fn count_age_files(dir: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += count_age_files(&path);
        } else if path.is_file() && crate::dot::encryption::is_encrypted_source(&path) {
            count += 1;
        }
    }
    count
}

pub(super) fn handle_repo_encryption(
    repo_name: &str,
    config: &mut DotfileConfig,
    db: &Database,
    debug: bool,
) -> Result<()> {
    let mut cursor = MenuCursor::new();

    loop {
        let mut actions = Vec::new();

        actions.push(RepoEncryptionItem {
            action: RepoEncryptionAction::ShowStatus,
            display: format!(
                "{} Show Status",
                format_icon_colored(NerdFont::InfoCircle, colors::BLUE)
            ),
            preview: PreviewBuilder::new()
                .line(colors::BLUE, Some(NerdFont::InfoCircle), "Show Status")
                .blank()
                .text("Display detailed authorized recipients and")
                .text("local decryption validation.")
                .build_string(),
        });

        actions.push(RepoEncryptionItem {
            action: RepoEncryptionAction::AuthorizeLocalKey,
            display: format!(
                "{} Authorize Local Key",
                format_icon_colored(NerdFont::Key, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .line(colors::GREEN, Some(NerdFont::Key), "Authorize Local Key")
                .blank()
                .text("Automatically authorize your local machine's primary")
                .text("key or discovered SSH keys in instantdots.toml")
                .text("and re-encrypt all files.")
                .build_string(),
        });

        actions.push(RepoEncryptionItem {
            action: RepoEncryptionAction::AuthorizeRemoteKey,
            display: format!(
                "{} Authorize Remote Key",
                format_icon_colored(NerdFont::Users, colors::PEACH)
            ),
            preview: PreviewBuilder::new()
                .line(colors::PEACH, Some(NerdFont::Users), "Authorize Remote Key")
                .blank()
                .text("Enter an external public key (age1... or ssh-...)")
                .text("to authorize a teammate or another device.")
                .build_string(),
        });

        actions.push(RepoEncryptionItem {
            action: RepoEncryptionAction::RotateKeys,
            display: format!(
                "{} Rotate Keys",
                format_icon_colored(NerdFont::Refresh, colors::RED)
            ),
            preview: PreviewBuilder::new()
                .line(colors::RED, Some(NerdFont::Refresh), "Rotate Keys")
                .blank()
                .text("Prompt for a comma-separated list of keys to set")
                .text("as the exclusive authorized recipients, performing")
                .text("safe key rotation.")
                .build_string(),
        });

        actions.push(RepoEncryptionItem {
            action: RepoEncryptionAction::Back,
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .subtext("Return to repository actions")
                .build_string(),
        });

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("Encryption: {}", repo_name)))
            .prompt("Select action")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&actions) {
            builder = builder.initial_index(index);
        }

        let result = builder.select(actions.clone())?;

        match result {
            FzfResult::Selected(item) => {
                cursor.update(&item, &actions);
                match item.action {
                    RepoEncryptionAction::ShowStatus => {
                        let msg = build_status_message(repo_name, config);
                        FzfWrapper::message(&msg)?;
                    }
                    RepoEncryptionAction::AuthorizeLocalKey => {
                        let local_keys = crate::dot::operations::key::get_local_public_keys()
                            .unwrap_or_default();
                        if local_keys.is_empty() {
                            let result = FzfWrapper::builder()
                                .responsive_layout()
                                .confirm(
                                    "No local age identity found.\n\nWould you like to generate one now?",
                                )
                                .yes_text("Generate Key")
                                .no_text("Cancel")
                                .confirm_dialog()?;
                            if result == crate::menu_utils::ConfirmResult::Yes {
                                crate::dot::operations::key::handle_init(false)?;
                                let new_keys = crate::dot::operations::key::get_local_public_keys()
                                    .unwrap_or_default();
                                if new_keys.is_empty() {
                                    FzfWrapper::message("No key was generated.")?;
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }
                        crate::dot::operations::key::handle_authorize(
                            config,
                            db,
                            None,
                            Some(repo_name),
                            false,
                            debug,
                        )?;
                        let local_keys = crate::dot::operations::key::get_local_public_keys()
                            .unwrap_or_default();
                        let key = local_keys
                            .first()
                            .map(|s| s.as_str())
                            .unwrap_or("(unknown)");
                        FzfWrapper::message(&format!(
                            "Local key authorized for {}.\n\n{}",
                            repo_name, key
                        ))?;
                    }
                    RepoEncryptionAction::AuthorizeRemoteKey => {
                        if let crate::menu_utils::TextEditOutcome::Updated(Some(key)) =
                            crate::menu_utils::prompt_text_edit(
                                crate::menu_utils::TextEditPrompt::new(
                                    "Enter public key to authorize (age1... or ssh-...):",
                                    None,
                                ),
                            )?
                        {
                            let key = key.trim();
                            if !key.is_empty() {
                                if !key.starts_with("age1") && !key.starts_with("ssh-") {
                                    FzfWrapper::message(
                                        "Invalid key prefix.\nExpected age1... or ssh-...",
                                    )?;
                                } else {
                                    crate::dot::operations::key::handle_authorize(
                                        config,
                                        db,
                                        Some(key),
                                        Some(repo_name),
                                        false,
                                        debug,
                                    )?;
                                    FzfWrapper::message(&format!(
                                        "Remote key authorized for {}.\n\n{}",
                                        repo_name, key
                                    ))?;
                                }
                            }
                        }
                    }
                    RepoEncryptionAction::RotateKeys => {
                        if let crate::menu_utils::TextEditOutcome::Updated(Some(keys_str)) =
                            crate::menu_utils::prompt_text_edit(
                                crate::menu_utils::TextEditPrompt::new(
                                    "Enter comma-separated public keys for rotation:",
                                    None,
                                ),
                            )?
                        {
                            let keys: Vec<String> = keys_str
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();

                            if !keys.is_empty() {
                                let mut has_invalid = false;
                                for k in &keys {
                                    if !k.starts_with("age1") && !k.starts_with("ssh-") {
                                        FzfWrapper::message(&format!(
                                            "Invalid key: {}\nExpected age1... or ssh-...",
                                            k
                                        ))?;
                                        has_invalid = true;
                                        break;
                                    }
                                }
                                if has_invalid {
                                    continue;
                                }

                                let confirm = FzfWrapper::confirm(&format!(
                                    "Rotate keys to {} recipient(s)?\nYou will lose access if your key is not included.",
                                    keys.len()
                                ))?;
                                if confirm == crate::menu_utils::ConfirmResult::Yes {
                                    crate::dot::operations::key::handle_rotate(
                                        config,
                                        db,
                                        &keys,
                                        Some(repo_name),
                                        false,
                                        debug,
                                    )?;
                                    FzfWrapper::message(&format!(
                                        "Keys rotated for {}.\nRecipients set: {}",
                                        repo_name,
                                        keys.len()
                                    ))?;
                                }
                            }
                        }
                    }
                    RepoEncryptionAction::Back => return Ok(()),
                }
            }
            FzfResult::Error(e) => return Err(anyhow::anyhow!("FZF Error: {}", e)),
            _ => return Ok(()),
        }
    }
}
