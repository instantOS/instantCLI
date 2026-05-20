use anyhow::Result;
use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::ui::prelude::*;

use super::discover::get_local_public_keys;
use super::rotate::reencrypt_repository;
use super::util::resolve_writable_repo;

pub fn handle_authorize(
    config: &DotfileConfig,
    db: &Database,
    recipient_opt: Option<&str>,
    repo_name_opt: Option<&str>,
    dry_run: bool,
    debug: bool,
) -> Result<()> {
    let (repo_name, repo_auto_selected) = resolve_writable_repo(
        config,
        repo_name_opt,
        "authorizing",
        "dot.key.authorize.repo_auto_selected",
    )?;

    let dotfile_repo = DotfileRepo::new(config, repo_name.clone())?;
    let repo_path = dotfile_repo.local_path(config)?;

    let recipient_key = if let Some(key) = recipient_opt {
        key.trim().to_string()
    } else {
        let local_pubkeys = get_local_public_keys()?;
        if local_pubkeys.is_empty() {
            anyhow::bail!(
                "No local encryption key found. Please run `ins dot keys generate` first to generate one."
            );
        } else if local_pubkeys.len() > 1 {
            anyhow::bail!(
                "Multiple local encryption keys found. Please specify which public key to authorize explicitly:\n{:?}",
                local_pubkeys
            );
        }
        local_pubkeys[0].clone()
    };

    if !recipient_key.starts_with("age1") && !recipient_key.starts_with("ssh-") {
        anyhow::bail!(
            "Invalid recipient public key: '{}'. Expected age1... or ssh-...",
            recipient_key
        );
    }

    let meta = crate::dot::meta::read_meta(&repo_path)?;

    if meta.encryption_recipients.contains(&recipient_key) {
        emit(
            Level::Info,
            "dot.key.authorize.already_present",
            &format!(
                "{} Recipient '{}' is already authorized in repository '{}'",
                char::from(NerdFont::Info),
                recipient_key.cyan(),
                repo_name
            ),
            None,
        );
        return Ok(());
    }

    let mut new_recipients = meta.encryption_recipients.clone();
    new_recipients.push(recipient_key.clone());

    reencrypt_repository(
        &repo_path,
        &dotfile_repo,
        &new_recipients,
        db,
        dry_run,
        debug,
    )?;

    let repo_note = if repo_auto_selected {
        " (auto-selected; pass --repo to override)".to_string()
    } else {
        String::new()
    };
    emit(
        Level::Success,
        "dot.key.authorize.success",
        &format!(
            "{} Successfully authorized recipient in repository '{}'{}!\n{} Recipient: {}\n{} Re-encrypted tracked files.",
            char::from(NerdFont::Check),
            repo_name,
            repo_note,
            char::from(NerdFont::Users),
            recipient_key.green(),
            char::from(NerdFont::Folder)
        ),
        Some(serde_json::json!({
            "repo": repo_name.as_str(),
            "auto_selected": repo_auto_selected,
        })),
    );

    Ok(())
}

pub fn handle_deauthorize(
    config: &DotfileConfig,
    db: &Database,
    recipient: &str,
    repo_name_opt: Option<&str>,
    dry_run: bool,
    debug: bool,
) -> Result<()> {
    let recipient_key = recipient.trim();
    if !recipient_key.starts_with("age1") && !recipient_key.starts_with("ssh-") {
        anyhow::bail!(
            "Invalid recipient public key: '{}'. Expected age1... or ssh-...",
            recipient_key
        );
    }

    let (repo_name, repo_auto_selected) = resolve_writable_repo(
        config,
        repo_name_opt,
        "de-authorizing",
        "dot.key.deauthorize.repo_auto_selected",
    )?;

    let dotfile_repo = DotfileRepo::new(config, repo_name.clone())?;
    let repo_path = dotfile_repo.local_path(config)?;
    let meta = crate::dot::meta::read_meta(&repo_path)?;

    if !meta
        .encryption_recipients
        .contains(&recipient_key.to_string())
    {
        emit(
            Level::Info,
            "dot.key.deauthorize.not_found",
            &format!(
                "{} Recipient '{}' is not authorized in repository '{}'",
                char::from(NerdFont::Info),
                recipient_key.cyan(),
                repo_name
            ),
            None,
        );
        return Ok(());
    }

    let local_pubkeys = get_local_public_keys()?;
    let is_self = local_pubkeys.contains(&recipient_key.to_string());

    if is_self && meta.encryption_recipients.len() == 1 {
        anyhow::bail!(
            "Self-lockout prevented: '{}' is the only authorized recipient and it belongs to you. \
             Add another key first or use --force if you really want to remove encryption.",
            recipient_key
        );
    }

    if meta.encryption_recipients.len() == 1 {
        emit(
            Level::Warn,
            "dot.key.deauthorize.last_recipient",
            &format!(
                "{} Removing the last authorized recipient. The repository will no longer be encrypted.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
    } else if is_self {
        emit(
            Level::Warn,
            "dot.key.deauthorize.self_removal",
            &format!(
                "{} You are de-authorizing your own key. You will lose the ability to decrypt \
                 files in this repository unless another of your keys is authorized.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
    }

    let new_recipients: Vec<String> = meta
        .encryption_recipients
        .into_iter()
        .filter(|r| r != recipient_key)
        .collect();

    reencrypt_repository(
        &repo_path,
        &dotfile_repo,
        &new_recipients,
        db,
        dry_run,
        debug,
    )?;

    let repo_note = if repo_auto_selected {
        " (auto-selected; pass --repo to override)"
    } else {
        ""
    };
    emit(
        Level::Success,
        "dot.key.deauthorize.success",
        &format!(
            "{} De-authorized recipient '{}' from repository '{}'{}!",
            char::from(NerdFont::Check),
            recipient_key.cyan(),
            repo_name,
            repo_note
        ),
        None,
    );

    Ok(())
}
