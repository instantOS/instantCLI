use anyhow::Result;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::menu::encryption_menu::{EncryptionKeyKind, discover_all_keys};
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{
    colors, format_back_icon, format_icon_colored, format_with_color, fzf_mocha_args,
};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Clone)]
pub(crate) enum MenuKind {
    Recipient { public_key: String, is_local: bool },
    AuthorizeLocal,
    AuthorizeRemote,
    Back,
}

#[derive(Clone)]
struct MenuItem {
    kind: MenuKind,
    display: String,
    preview: String,
}

impl FzfSelectable for MenuItem {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        match &self.kind {
            MenuKind::Recipient { public_key, .. } => public_key.clone(),
            MenuKind::AuthorizeLocal => "authorize_local".to_string(),
            MenuKind::AuthorizeRemote => "authorize_remote".to_string(),
            MenuKind::Back => "back".to_string(),
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
    let display_server = crate::common::display_server::DisplayServer::detect();

    loop {
        let Ok(dotfile_repo) =
            crate::dot::dotfilerepo::DotfileRepo::new(config, repo_name.to_string())
        else {
            FzfWrapper::message("Repository not found.")?;
            return Ok(());
        };
        let Ok(repo_path) = dotfile_repo.local_path(config) else {
            FzfWrapper::message("Repository path not found.")?;
            return Ok(());
        };
        let Ok(meta) = crate::dot::meta::read_meta(&repo_path) else {
            FzfWrapper::message("Could not read repository metadata.")?;
            return Ok(());
        };

        let local_keys = discover_all_keys();
        let local_key_map: std::collections::HashMap<&str, &EncryptionKeyKind> =
            local_keys.iter().map(|k| (k.public_key(), k)).collect();
        let mut items: Vec<MenuItem> = Vec::new();

        for r in &meta.encryption_recipients {
            let matched = local_key_map.get(r.as_str()).copied();
            let is_local = matched.is_some();
            let icon = format_icon_colored(
                if is_local {
                    NerdFont::CheckCircle
                } else {
                    NerdFont::Globe
                },
                if is_local {
                    colors::GREEN
                } else {
                    colors::PEACH
                },
            );

            let display_label = if let Some(key) = matched {
                format!("{}  ({})", key.display_name(), key.key_type_label())
            } else {
                let short = {
                    const MAX: usize = 40;
                    if r.len() > MAX {
                        format!("{}...", &r[..MAX.saturating_sub(3)])
                    } else {
                        r.clone()
                    }
                };
                format!("{}  (Remote)", short)
            };

            let mut preview = PreviewBuilder::new()
                .header(NerdFont::Users, "Authorized Recipient")
                .blank();

            if let Some(key) = matched {
                preview = preview
                    .field("Name", &key.display_name())
                    .field("Type", key.key_type_label())
                    .field("Public key", r)
                    .field("Path", &key.path().to_string_lossy());

                if let Ok(meta) = std::fs::metadata(key.path())
                    && let Ok(modified) = meta.modified()
                    && let Ok(duration) = modified.elapsed()
                {
                    let days = duration.as_secs() / 86400;
                    let date_str = if days == 0 {
                        "Today".to_string()
                    } else if days == 1 {
                        "Yesterday".to_string()
                    } else {
                        format!("{} days ago", days)
                    };
                    preview = preview.field("Created", &date_str);
                }

                let authorized_repos =
                    crate::dot::operations::key::find_repos_using_key(config, key.public_key());
                if !authorized_repos.is_empty() {
                    preview = preview
                        .blank()
                        .field("Authorized in", &authorized_repos.join(", "));
                }
            } else {
                preview = preview.field("Type", "Remote Key").field("Public key", r);
            }

            items.push(MenuItem {
                kind: MenuKind::Recipient {
                    public_key: r.clone(),
                    is_local,
                },
                display: format!("{} {}", icon, display_label),
                preview: preview.build_string(),
            });
        }

        items.push(MenuItem {
            kind: MenuKind::AuthorizeLocal,
            display: format!(
                "{} Authorize Local Key",
                format_icon_colored(NerdFont::Key, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Key, "Authorize Local Key")
                .blank()
                .text("Authorize your local machine's encryption key.")
                .text("Files will be re-encrypted for the new recipient set.")
                .build_string(),
        });

        items.push(MenuItem {
            kind: MenuKind::AuthorizeRemote,
            display: format!(
                "{} Authorize Remote Key",
                format_icon_colored(NerdFont::Users, colors::PEACH)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Users, "Authorize Remote Key")
                .blank()
                .text("Enter an external public key (age1... or ssh-...)")
                .text("to authorize a teammate or another device.")
                .build_string(),
        });

        items.push(MenuItem {
            kind: MenuKind::Back,
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .subtext("Return to repository actions")
                .build_string(),
        });

        let builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("Encryption: {}", repo_name)))
            .prompt("Select action")
            .args(fzf_mocha_args())
            .responsive_layout();

        let result = builder.select(items)?;

        match result {
            FzfResult::Selected(item) => match &item.kind {
                MenuKind::Recipient {
                    public_key,
                    is_local,
                } => {
                    handle_recipient_actions(
                        public_key,
                        *is_local,
                        repo_name,
                        config,
                        db,
                        debug,
                        &display_server,
                    )?;
                }
                MenuKind::AuthorizeLocal => {
                    let mut keys = discover_all_keys();

                    if keys.is_empty() {
                        let result = FzfWrapper::builder()
                            .responsive_layout()
                            .confirm(
                                "No local encryption key found.\n\nWould you like to generate one now?",
                            )
                            .yes_text("Generate Key")
                            .no_text("Cancel")
                            .confirm_dialog()?;
                        if result == crate::menu_utils::ConfirmResult::Yes {
                            crate::dot::operations::key::handle_init(None, false)?;
                            keys = discover_all_keys();
                            if keys.is_empty() {
                                FzfWrapper::message("No key was generated.")?;
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }

                    let chosen: EncryptionKeyKind = if keys.len() == 1 {
                        keys[0].clone()
                    } else {
                        #[derive(Clone)]
                        enum PickerAction {
                            Select(EncryptionKeyKind),
                            Generate,
                            Back,
                        }

                        #[derive(Clone)]
                        struct PickerEntry {
                            action: PickerAction,
                            display: String,
                            preview: String,
                        }

                        impl FzfSelectable for PickerEntry {
                            fn fzf_display_text(&self) -> String {
                                self.display.clone()
                            }
                            fn fzf_key(&self) -> String {
                                match &self.action {
                                    PickerAction::Select(k) => k.public_key().to_string(),
                                    PickerAction::Generate => "generate".to_string(),
                                    PickerAction::Back => "back".to_string(),
                                }
                            }
                            fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
                                crate::menu::protocol::FzfPreview::Text(self.preview.clone())
                            }
                        }

                        let mut entries: Vec<PickerEntry> = keys
                            .iter()
                            .map(|key| {
                                let icon = format_icon_colored(
                                    match key {
                                        EncryptionKeyKind::AgeIdentity { .. } => NerdFont::Lock,
                                        EncryptionKeyKind::SshKey { .. } => NerdFont::Terminal,
                                    },
                                    match key {
                                        EncryptionKeyKind::AgeIdentity { .. } => colors::GREEN,
                                        EncryptionKeyKind::SshKey { .. } => colors::BLUE,
                                    },
                                );
                                let authorized_repos =
                                    crate::dot::operations::key::find_repos_using_key(
                                        config,
                                        key.public_key(),
                                    );
                                let auth_info = if authorized_repos.is_empty() {
                                    "Not authorized in any repo".to_string()
                                } else {
                                    format!("Authorized in: {}", authorized_repos.join(", "))
                                };
                                PickerEntry {
                                    action: PickerAction::Select(key.clone()),
                                    display: format!(
                                        "{} {}  {}",
                                        icon,
                                        key.display_name(),
                                        format_with_color(&key.short_key(), colors::OVERLAY0),
                                    ),
                                    preview: PreviewBuilder::new()
                                        .header(
                                            NerdFont::Key,
                                            &format!("{} Key", key.key_type_label()),
                                        )
                                        .blank()
                                        .field("Public key", key.public_key())
                                        .field("Path", &key.path().to_string_lossy())
                                        .blank()
                                        .text(&auth_info)
                                        .build_string(),
                                }
                            })
                            .collect();

                        entries.push(PickerEntry {
                            action: PickerAction::Generate,
                            display: format!(
                                "{} Generate New Key",
                                format_icon_colored(NerdFont::Plus, colors::GREEN)
                            ),
                            preview: PreviewBuilder::new()
                                .header(NerdFont::Plus, "Generate New Key")
                                .text("Create a new x25519 encryption keypair.")
                                .blank()
                                .subtext("Existing keys are not overwritten.")
                                .build_string(),
                        });

                        entries.push(PickerEntry {
                            action: PickerAction::Back,
                            display: format!("{} Back", format_back_icon()),
                            preview: PreviewBuilder::new()
                                .subtext("Return to repository encryption menu")
                                .build_string(),
                        });

                        let builder = FzfWrapper::builder()
                            .header(Header::fancy("Select Key to Authorize"))
                            .prompt("Key")
                            .responsive_layout();

                        match builder.select(entries)? {
                            FzfResult::Selected(entry) => match &entry.action {
                                PickerAction::Select(k) => k.clone(),
                                PickerAction::Generate => {
                                    let default_name = nix::unistd::gethostname()
                                        .ok()
                                        .and_then(|h| h.into_string().ok())
                                        .unwrap_or_else(|| "default".to_string());
                                    let name = FzfWrapper::builder()
                                        .header(Header::fancy("Generate New Key"))
                                        .prompt("Key name")
                                        .query(&default_name)
                                        .input()
                                        .input_dialog()?;
                                    if !name.is_empty() {
                                        crate::dot::operations::key::handle_init(
                                            Some(&name),
                                            false,
                                        )?;
                                    }
                                    continue;
                                }
                                PickerAction::Back => continue,
                            },
                            _ => continue,
                        }
                    };

                    crate::dot::operations::key::handle_authorize(
                        config,
                        db,
                        Some(chosen.public_key()),
                        Some(repo_name),
                        false,
                        debug,
                    )?;

                    FzfWrapper::message(&format!(
                        "Local key '{}' authorized for {}.\n\n{}",
                        chosen.display_name(),
                        repo_name,
                        chosen.public_key(),
                    ))?;
                }
                MenuKind::AuthorizeRemote => {
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
                MenuKind::Back => return Ok(()),
            },
            _ => return Ok(()),
        }
    }
}

fn handle_recipient_actions(
    recipient_key: &str,
    is_local: bool,
    repo_name: &str,
    config: &mut DotfileConfig,
    db: &Database,
    debug: bool,
    display_server: &crate::common::display_server::DisplayServer,
) -> Result<()> {
    let short_key = {
        const MAX: usize = 40;
        if recipient_key.len() > MAX {
            format!("{}...", &recipient_key[..MAX.saturating_sub(3)])
        } else {
            recipient_key.to_string()
        }
    };

    loop {
        #[derive(Clone)]
        struct ActionItem {
            action: &'static str,
            display: String,
            preview: String,
        }

        impl FzfSelectable for ActionItem {
            fn fzf_display_text(&self) -> String {
                self.display.clone()
            }
            fn fzf_key(&self) -> String {
                self.action.to_string()
            }
            fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
                crate::menu::protocol::FzfPreview::Text(self.preview.clone())
            }
        }

        let type_label = if is_local { "Your Key" } else { "Remote Key" };

        let items = vec![
            ActionItem {
                action: "deauthorize",
                display: format!(
                    "{} De-authorize",
                    format_icon_colored(NerdFont::Minus, colors::RED)
                ),
                preview: PreviewBuilder::new()
                    .line(colors::RED, Some(NerdFont::Minus), "De-authorize")
                    .blank()
                    .text(&format!(
                        "Remove this recipient ({}) from the repository.",
                        type_label
                    ))
                    .blank()
                    .text("Files will be re-encrypted for the remaining recipients.")
                    .build_string(),
            },
            ActionItem {
                action: "copy",
                display: format!(
                    "{} Copy Public Key",
                    format_icon_colored(NerdFont::Clipboard, colors::GREEN)
                ),
                preview: PreviewBuilder::new()
                    .header(NerdFont::Clipboard, "Copy Public Key")
                    .blank()
                    .field("Key", recipient_key)
                    .build_string(),
            },
            ActionItem {
                action: "back",
                display: format!("{} Back", format_back_icon()),
                preview: PreviewBuilder::new()
                    .subtext("Return to repository encryption menu")
                    .build_string(),
            },
        ];

        let builder = FzfWrapper::builder()
            .header(Header::fancy(&format!("{} — {}", type_label, short_key)))
            .prompt("Action")
            .responsive_layout();

        let result = builder.select(items)?;
        match result {
            FzfResult::Selected(item) => match item.action {
                "deauthorize" => {
                    let recipients = {
                        let Ok(dotfile_repo) = crate::dot::dotfilerepo::DotfileRepo::new(
                            config,
                            repo_name.to_string(),
                        ) else {
                            FzfWrapper::message("Repository not found.")?;
                            return Ok(());
                        };
                        let Ok(repo_path) = dotfile_repo.local_path(config) else {
                            FzfWrapper::message("Repository path not found.")?;
                            return Ok(());
                        };
                        let Ok(meta) = crate::dot::meta::read_meta(&repo_path) else {
                            FzfWrapper::message("Could not read repository metadata.")?;
                            return Ok(());
                        };
                        meta.encryption_recipients
                    };

                    let warning = if is_local && recipients.len() == 1 {
                        "\n\u{26a0} WARNING: This is your only key and the only recipient.\nRemoving it will leave the repository unencrypted.".to_string()
                    } else if is_local {
                        "\n\u{26a0} WARNING: This is your own key.\nYou will lose the ability to decrypt unless another of your keys is authorized.".to_string()
                    } else {
                        String::new()
                    };

                    let confirm_result = FzfWrapper::builder()
                        .responsive_layout()
                        .confirm(format!(
                            "Remove this recipient from '{}'?\n\n{}{}",
                            repo_name, short_key, warning
                        ))
                        .yes_text("De-authorize")
                        .no_text("Cancel")
                        .confirm_dialog()?;
                    if confirm_result != crate::menu_utils::ConfirmResult::Yes {
                        continue;
                    }

                    crate::dot::operations::key::handle_deauthorize(
                        config,
                        db,
                        recipient_key,
                        Some(repo_name),
                        false,
                        debug,
                    )?;

                    FzfWrapper::message(&format!(
                        "Recipient de-authorized from {}.\n\n{}",
                        repo_name, recipient_key
                    ))?;
                    return Ok(());
                }
                "copy" => {
                    crate::assist::utils::copy_to_clipboard(
                        recipient_key.as_bytes(),
                        display_server,
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to copy to clipboard: {}", e))?;
                    FzfWrapper::message(&format!(
                        "Public key copied to clipboard.\n\n{}",
                        recipient_key
                    ))?;
                }
                _ => return Ok(()),
            },
            _ => return Ok(()),
        }
    }
}
