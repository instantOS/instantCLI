use anyhow::{Context, Result};
use std::path::Path;

use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::ui::prelude::*;

use super::discover::get_local_public_keys;
use super::util::{find_age_files, resolve_writable_repo};

pub fn handle_rotate(
    config: &DotfileConfig,
    db: &Database,
    recipients: &[String],
    repo_name_opt: Option<&str>,
    dry_run: bool,
    debug: bool,
) -> Result<()> {
    if recipients.is_empty() {
        anyhow::bail!("You must specify at least one recipient key for key rotation.");
    }
    for recipient_key in recipients {
        if !recipient_key.starts_with("age1") && !recipient_key.starts_with("ssh-") {
            anyhow::bail!(
                "Invalid recipient public key: '{}'. Expected age1... or ssh-...",
                recipient_key
            );
        }
    }

    let local_pubkeys = get_local_public_keys()?;
    let mut lockout = true;
    for pk in &local_pubkeys {
        if recipients.contains(pk) {
            lockout = false;
            break;
        }
    }

    if lockout && !local_pubkeys.is_empty() {
        anyhow::bail!(
            "Self-lockout prevented: Your local public keys are not in the new recipient list. You would lose access. To rotate, ensure your key is included."
        );
    }

    let (repo_name, repo_auto_selected) = resolve_writable_repo(
        config,
        repo_name_opt,
        "rotating",
        "dot.key.rotate.repo_auto_selected",
    )?;

    let dotfile_repo = DotfileRepo::new(config, repo_name.clone())?;
    let repo_path = dotfile_repo.local_path(config)?;

    reencrypt_repository(&repo_path, &dotfile_repo, recipients, db, dry_run, debug)?;

    let repo_note = if repo_auto_selected {
        " (auto-selected; pass --repo to override)"
    } else {
        ""
    };
    emit(
        Level::Success,
        "dot.key.rotate.success",
        &format!(
            "{} Successfully rotated keys in repository '{}'{}!\n{} New Recipients: {:?}",
            char::from(NerdFont::Check),
            repo_name,
            repo_note,
            char::from(NerdFont::Users),
            recipients
        ),
        Some(serde_json::json!({
            "repo": repo_name.as_str(),
            "auto_selected": repo_auto_selected,
        })),
    );

    Ok(())
}

/// Shared re-encryption logic used by authorize, deauthorize, and rotate.
///
/// Decrypts all `.age` files in the repository with current identities,
/// re-encrypts them with the new recipient set, writes atomically, and
/// updates the database mappings.
pub fn reencrypt_repository(
    repo_path: &Path,
    dotfile_repo: &DotfileRepo,
    new_recipients_str: &[String],
    db: &Database,
    dry_run: bool,
    debug: bool,
) -> Result<()> {
    let mut meta = crate::dot::meta::read_meta(repo_path)?;

    let mut encrypted_files = Vec::new();
    for dir in &dotfile_repo.dotfile_dirs {
        if dir.path.is_dir() {
            find_age_files(&dir.path, &mut encrypted_files)?;
        }
    }

    let identities = crate::dot::encryption::load_identities()?;
    if !encrypted_files.is_empty() && identities.is_empty() {
        anyhow::bail!(
            "Repository has existing encrypted files, but no local encryption keys were found. Please configure your key first."
        );
    }

    if dry_run {
        println!("--- DRY RUN ---");
        println!(
            "Would rotate/authorize recipients to: {:?}",
            new_recipients_str
        );
        println!(
            "Would update instantdots.toml at: {}",
            repo_path.join("instantdots.toml").display()
        );
        println!("Would re-encrypt {} tracked files:", encrypted_files.len());
        for file in &encrypted_files {
            println!("  - {}", file.display());
        }
        return Ok(());
    }

    let parsed_recipients = crate::dot::encryption::parse_recipients(new_recipients_str)?;

    let mut pending_writes = Vec::new();
    for file in &encrypted_files {
        let old_cipher_hash = Dotfile::compute_hash(file).with_context(|| {
            format!(
                "hashing existing encrypted file {} before re-encrypt",
                file.display()
            )
        })?;
        let plain_bytes = crate::dot::encryption::decrypt_file_to_bytes(file, &identities)
            .with_context(|| {
                format!(
                    "Decryption failed for '{}'. You must possess a valid identity key matching the existing recipients before modifying recipients.",
                    file.display()
                )
            })?;
        let plain_hash = Dotfile::hash_bytes(&plain_bytes);
        let cipher_bytes =
            crate::dot::encryption::encrypt_bytes_to_armored(&plain_bytes, &parsed_recipients)?;
        let new_cipher_hash = Dotfile::hash_bytes(&cipher_bytes);
        pending_writes.push((
            file,
            cipher_bytes,
            plain_hash,
            new_cipher_hash,
            old_cipher_hash,
        ));
    }

    // Phase 2: Write all files atomically and update database
    for (file, cipher_bytes, plain_hash, new_cipher_hash, old_cipher_hash) in pending_writes {
        crate::dot::utils::persist_file_safely(file, &cipher_bytes, "encrypted file")?;
        if old_cipher_hash != new_cipher_hash {
            db.delete_encrypted_source(&old_cipher_hash)?;
        }
        db.record_encrypted_source(&new_cipher_hash, &plain_hash)?;
        crate::dot::git::repo_ops::git_add(repo_path, file, debug)?;
    }

    meta.encryption_recipients = new_recipients_str.to_vec();
    crate::dot::meta::update_meta(repo_path, &meta)?;
    crate::dot::git::repo_ops::git_add(repo_path, &repo_path.join("instantdots.toml"), debug)?;

    Ok(())
}
