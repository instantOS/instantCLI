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
                        crate::dot::operations::key::handle_status(config, Some(repo_name))?;
                        let _ = crate::menu_utils::prompt_text_edit(
                            crate::menu_utils::TextEditPrompt::new("Press Enter to continue", None),
                        )?;
                    }
                    RepoEncryptionAction::AuthorizeLocalKey => {
                        crate::dot::operations::key::handle_authorize(
                            config,
                            db,
                            None,
                            Some(repo_name),
                            false,
                            debug,
                        )?;
                        let _ = crate::menu_utils::prompt_text_edit(
                            crate::menu_utils::TextEditPrompt::new("Press Enter to continue", None),
                        )?;
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
                                    let _ = crate::menu_utils::prompt_text_edit(
                                        crate::menu_utils::TextEditPrompt::new(
                                            "Invalid key prefix. Press Enter to continue",
                                            None,
                                        ),
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
                                    let _ = crate::menu_utils::prompt_text_edit(
                                        crate::menu_utils::TextEditPrompt::new(
                                            "Press Enter to continue",
                                            None,
                                        ),
                                    )?;
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
                                for k in &keys {
                                    if !k.starts_with("age1") && !k.starts_with("ssh-") {
                                        let _ = crate::menu_utils::prompt_text_edit(
                                            crate::menu_utils::TextEditPrompt::new(
                                                &format!(
                                                    "Invalid key '{}'. Press Enter to continue",
                                                    k
                                                ),
                                                None,
                                            ),
                                        )?;
                                        continue;
                                    }
                                }

                                let confirm = crate::menu_utils::FzfWrapper::confirm(&format!(
                                    "Are you sure you want to rotate keys to {} recipients? You will lose access if your key is not included.",
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
                                    let _ = crate::menu_utils::prompt_text_edit(
                                        crate::menu_utils::TextEditPrompt::new(
                                            "Press Enter to continue",
                                            None,
                                        ),
                                    )?;
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
