//! Encryption key and recipient management operations.

use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::dot::commands::EncryptCommands;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use crate::dot::dotfile::Dotfile;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::ui::prelude::*;

/// Primary entry point for executing encryption subcommands.
pub fn handle_encrypt_command(
    config: &DotfileConfig,
    db: &Database,
    command: &EncryptCommands,
    debug: bool,
) -> Result<()> {
    match command {
        EncryptCommands::Generate { name, force } => handle_init(name.as_deref(), *force),
        EncryptCommands::List => handle_list(config),
        EncryptCommands::Rename { old_name, new_name } => handle_rename(old_name, new_name),
        EncryptCommands::Remove { name } => handle_remove(config, name),
        EncryptCommands::Authorize {
            recipient,
            repo,
            dry_run,
            ..
        } => handle_authorize(
            config,
            db,
            recipient.as_deref(),
            repo.as_deref(),
            *dry_run,
            debug,
        ),
        EncryptCommands::Deauthorize {
            recipient,
            repo,
            dry_run,
            ..
        } => handle_deauthorize(config, db, recipient, repo.as_deref(), *dry_run, debug),
        EncryptCommands::Rotate {
            recipients,
            repo,
            dry_run,
            ..
        } => handle_rotate(config, db, recipients, repo.as_deref(), *dry_run, debug),
        EncryptCommands::Status { repo, .. } => handle_status(config, repo.as_deref()),
        EncryptCommands::Show => handle_identity(),
    }
}

/// Get the identities directory path, creating it if needed.
fn identities_dir() -> Result<PathBuf> {
    let config_dir = crate::common::paths::instant_config_dir()?;
    let dir = config_dir.join("encryption").join("identities");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Validate a key name: non-empty, no path separators, no `.`/`..`.
fn validate_key_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Key name cannot be empty");
    }
    if name.contains('/') || name.contains(std::path::MAIN_SEPARATOR) {
        anyhow::bail!("Key name cannot contain path separators");
    }
    if name == "." || name == ".." {
        anyhow::bail!("Key name cannot be '.' or '..'");
    }
    Ok(())
}

pub(crate) fn handle_init(name: Option<&str>, force: bool) -> Result<()> {
    let key_name = match name {
        Some(n) => {
            validate_key_name(n)?;
            n.to_string()
        }
        None => {
            let default = nix::unistd::gethostname()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "default".to_string());
            default
        }
    };

    let dir = identities_dir()?;
    let identity_path = dir.join(&key_name);

    if identity_path.exists() && !force {
        let content = fs::read_to_string(&identity_path)?;
        let mut pubkey = None;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("AGE-SECRET-KEY-1")
                && let Ok(identity) = age::x25519::Identity::from_str(trimmed)
            {
                pubkey = Some(identity.to_public().to_string());
            }
        }
        if let Some(pk) = pubkey {
            emit(
                Level::Info,
                "dot.key.init.exists",
                &format!(
                    "{} Encryption key '{}' already exists!\n{} Path: {}\n{} Public recipient key: {}",
                    char::from(NerdFont::Info),
                    key_name.cyan(),
                    char::from(NerdFont::Lock),
                    identity_path.display().to_string().cyan(),
                    char::from(NerdFont::Users),
                    pk.green()
                ),
                None,
            );
        } else {
            anyhow::bail!(
                "Encryption key file already exists at {} but is invalid or corrupted. Use --force to overwrite it.",
                identity_path.display()
            );
        }
        return Ok(());
    }

    emit(
        Level::Info,
        "dot.key.init.generating",
        "Generating secure encryption keypair...",
        None,
    );

    let identity = age::x25519::Identity::generate();
    let public_key = identity.to_public().to_string();

    use age::secrecy::ExposeSecret;
    let secret_string = identity.to_string();
    let secret_str = secret_string.expose_secret();

    crate::dot::utils::persist_file_safely(
        &identity_path,
        secret_str.as_bytes(),
        "encryption key",
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&identity_path, fs::Permissions::from_mode(0o600))?;
    }

    emit(
        Level::Success,
        "dot.key.init.success",
        &format!(
            "{} Generated secure encryption keypair '{}'\n{} Private key saved to: {}\n{} Public key: {}",
            char::from(NerdFont::Check),
            key_name.cyan(),
            char::from(NerdFont::Lock),
            identity_path.display().to_string().cyan(),
            char::from(NerdFont::Users),
            public_key.green()
        ),
        None,
    );

    Ok(())
}

pub(crate) fn handle_authorize(
    config: &DotfileConfig,
    db: &Database,
    recipient_opt: Option<&str>,
    repo_name_opt: Option<&str>,
    dry_run: bool,
    debug: bool,
) -> Result<()> {
    let (repo_name, repo_auto_selected) = if let Some(name) = repo_name_opt {
        (name.to_string(), false)
    } else {
        let writable_repos = config.get_writable_repos();
        if writable_repos.is_empty() {
            anyhow::bail!("No writable repositories found in config to authorize keys.");
        }
        let chosen = writable_repos[0].name.clone();
        if writable_repos.len() > 1 {
            // Make the implicit choice visible so users with multiple repos
            // don't accidentally authorize the wrong one (issue #9).
            let other_names: Vec<&str> = writable_repos
                .iter()
                .skip(1)
                .map(|r| r.name.as_str())
                .collect();
            emit(
                Level::Warn,
                "dot.key.authorize.repo_auto_selected",
                &format!(
                    "{} No --repo given; authorizing in '{}' (other writable repos: {}). \
                     Pass --repo to choose explicitly.",
                    char::from(NerdFont::Warning),
                    chosen.cyan(),
                    other_names.join(", ")
                ),
                Some(serde_json::json!({
                    "selected_repo": chosen,
                    "other_writable_repos": other_names,
                })),
            );
        }
        (chosen, true)
    };

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

pub(crate) fn handle_deauthorize(
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

    let (repo_name, repo_auto_selected) = if let Some(name) = repo_name_opt {
        (name.to_string(), false)
    } else {
        let writable_repos = config.get_writable_repos();
        if writable_repos.is_empty() {
            anyhow::bail!("No writable repositories found in config to de-authorize keys.");
        }
        let chosen = writable_repos[0].name.clone();
        if writable_repos.len() > 1 {
            let other_names: Vec<&str> = writable_repos
                .iter()
                .skip(1)
                .map(|r| r.name.as_str())
                .collect();
            emit(
                Level::Warn,
                "dot.key.deauthorize.repo_auto_selected",
                &format!(
                    "{} No --repo given; de-authorizing in '{}' (other writable repos: {}). \
                     Pass --repo to choose explicitly.",
                    char::from(NerdFont::Warning),
                    chosen.cyan(),
                    other_names.join(", ")
                ),
                None,
            );
        }
        (chosen, true)
    };

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

pub(crate) fn handle_rotate(
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

    let (repo_name, repo_auto_selected) = if let Some(name) = repo_name_opt {
        (name.to_string(), false)
    } else {
        let writable_repos = config.get_writable_repos();
        if writable_repos.is_empty() {
            anyhow::bail!("No writable repositories found in config to rotate keys.");
        }
        let chosen = writable_repos[0].name.clone();
        if writable_repos.len() > 1 {
            // Issue #9 — surface the implicit choice; rotate is destructive
            // so be loud about which repo got picked.
            let other_names: Vec<&str> = writable_repos
                .iter()
                .skip(1)
                .map(|r| r.name.as_str())
                .collect();
            emit(
                Level::Warn,
                "dot.key.rotate.repo_auto_selected",
                &format!(
                    "{} No --repo given; rotating in '{}' (other writable repos: {}). \
                     Pass --repo to choose explicitly.",
                    char::from(NerdFont::Warning),
                    chosen.cyan(),
                    other_names.join(", ")
                ),
                Some(serde_json::json!({
                    "selected_repo": chosen,
                    "other_writable_repos": other_names,
                })),
            );
        }
        (chosen, true)
    };

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

fn reencrypt_repository(
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

    // Decrypt and re-encrypt all files in memory first.
    // This prevents a partial failure from leaving the repository in a mixed state.
    // Capture the OLD cipher hash for each file so we can prune its
    // encrypted_sources row after the new ciphertext is in place (issue
    // #7 — without this, the cipher→plain mapping table grows unboundedly
    // on every key rotation/authorize).
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
        // Drop the stale mapping first so the table doesn't accumulate
        // orphaned rows on every recipient change. Skip if the encryption
        // happened to produce identical ciphertext (no-op rotation).
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

pub(crate) fn handle_status(config: &DotfileConfig, target_repo_opt: Option<&str>) -> Result<()> {
    let writable_repos = config.get_writable_repos();
    if writable_repos.is_empty() {
        emit(
            Level::Info,
            "dot.key.status.no_repos",
            &format!(
                "{} No writable repositories configured",
                char::from(NerdFont::Info)
            ),
            None,
        );
        return Ok(());
    }

    let local_pubkeys = get_local_public_keys()?;

    let repos_to_check = if let Some(target_repo) = target_repo_opt {
        let found = writable_repos.iter().find(|r| r.name == target_repo);
        match found {
            Some(r) => vec![(*r).clone()],
            None => anyhow::bail!(
                "Repository '{}' not found in config or is read-only",
                target_repo
            ),
        }
    } else {
        writable_repos.iter().map(|r| (*r).clone()).collect()
    };

    println!(
        "{} Repository Age Key Status Dashboard (Local identities: {})",
        char::from(NerdFont::ShieldLock).to_string().cyan(),
        local_pubkeys.len()
    );
    println!(
        "{}",
        "────────────────────────────────────────────────────────────────────────────────".cyan()
    );

    for repo in repos_to_check {
        let dotfile_repo = DotfileRepo::new(config, repo.name.clone())?;
        let repo_path = dotfile_repo.local_path(config)?;
        let meta = crate::dot::meta::read_meta(&repo_path)?;
        let recipients = &meta.encryption_recipients;

        println!(
            "{} Repository: {} (Path: {})",
            char::from(NerdFont::Folder).to_string().blue(),
            repo.name.cyan(),
            repo_path.display()
        );
        if recipients.is_empty() {
            println!(
                "  {} Encryption Status: Not Encrypted",
                char::from(NerdFont::InfoCircle).to_string().dimmed()
            );
            println!();
            continue;
        }

        println!(
            "  {} Configured Recipients:",
            char::from(NerdFont::Users).to_string().cyan()
        );
        let mut matching_identity_found = false;
        for recipient in recipients {
            let is_local = local_pubkeys.contains(recipient);
            if is_local {
                matching_identity_found = true;
                println!(
                    "     {} {}  (Authorized)",
                    char::from(NerdFont::CheckCircle).to_string().green(),
                    recipient.green()
                );
            } else {
                println!(
                    "     {} {}  (Remote Identity)",
                    char::from(NerdFont::Lock).to_string().yellow(),
                    recipient.yellow()
                );
            }
        }

        let mut encrypted_files = Vec::new();
        for dir in &dotfile_repo.dotfile_dirs {
            if dir.path.is_dir() {
                find_age_files(&dir.path, &mut encrypted_files)?;
            }
        }

        println!(
            "  {} Encrypted Files Tracked: {}",
            char::from(NerdFont::FolderConfig).to_string().blue(),
            encrypted_files.len()
        );
        if !encrypted_files.is_empty() {
            if matching_identity_found {
                println!(
                    "  {} Local Decryption Status: {} Authorized!",
                    char::from(NerdFont::Key).to_string().green(),
                    char::from(NerdFont::Check).to_string().green()
                );
            } else {
                println!(
                    "  {} Local Decryption Status: {} Unauthorized!",
                    char::from(NerdFont::ShieldAlert).to_string().red(),
                    char::from(NerdFont::Warning).to_string().red()
                );
                println!(
                    "     {} Hint: Place the matching private key in ~/.config/instant/encryption/identities/",
                    char::from(NerdFont::Lightbulb).to_string().yellow()
                );
            }
        }
        println!();
    }

    Ok(())
}

#[derive(Debug)]
pub struct KeyInfo {
    pub name: String,
    pub key_type: KeyType,
    pub public_key: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Age,
    Ssh,
}

impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyType::Age => write!(f, "age"),
            KeyType::Ssh => write!(f, "ssh"),
        }
    }
}

pub fn discover_all_keys_info() -> Result<Vec<KeyInfo>> {
    let mut keys = Vec::new();

    for path in crate::dot::encryption::discover_identity_files() {
        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("AGE-SECRET-KEY-1")
                    && let Ok(identity) = age::x25519::Identity::from_str(trimmed)
                {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.to_string_lossy().to_string());
                    keys.push(KeyInfo {
                        name,
                        key_type: KeyType::Age,
                        public_key: identity.to_public().to_string(),
                        path: path.clone(),
                    });
                }
            }
        }
    }

    let home = std::env::var("HOME").map(PathBuf::from).ok();
    if let Some(home_path) = home {
        let ssh_dir = home_path.join(".ssh");
        if ssh_dir.is_dir()
            && let Ok(entries) = fs::read_dir(&ssh_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && path.extension().is_some_and(|ext| ext == "pub")
                    && let Ok(content) = fs::read_to_string(&path)
                {
                    let content_trimmed = content.trim();
                    if content_trimmed.starts_with("ssh-") || content_trimmed.starts_with("ecdsa-")
                    {
                        let name = path
                            .file_stem()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.to_string_lossy().to_string());
                        keys.push(KeyInfo {
                            name,
                            key_type: KeyType::Ssh,
                            public_key: content_trimmed.to_string(),
                            path,
                        });
                    }
                }
            }
        }
    }

    Ok(keys)
}

fn handle_list(config: &DotfileConfig) -> Result<()> {
    let keys = discover_all_keys_info()?;

    if keys.is_empty() {
        emit(
            Level::Info,
            "dot.key.list.empty",
            &format!(
                "{} No encryption keys found.\n{} Run `ins dot keys generate` to create one.",
                char::from(NerdFont::Info),
                char::from(NerdFont::Lightbulb).to_string().yellow()
            ),
            None,
        );
        return Ok(());
    }

    // Collect authorized repos for each key
    let writable_repos = config.get_writable_repos();
    let mut key_repo_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for repo in &writable_repos {
        if let Ok(dotfile_repo) = DotfileRepo::new(config, repo.name.clone())
            && let Ok(repo_path) = dotfile_repo.local_path(config)
            && let Ok(meta) = crate::dot::meta::read_meta(&repo_path)
        {
            for recipient in &meta.encryption_recipients {
                key_repo_map
                    .entry(recipient.clone())
                    .or_default()
                    .push(repo.name.clone());
            }
        }
    }

    println!(
        "{} Local Encryption Keys ({} found)",
        char::from(NerdFont::Key).to_string().cyan(),
        keys.len()
    );
    println!(
        "{}",
        "────────────────────────────────────────────────────────────────────────────────".cyan()
    );

    for key in &keys {
        let short_key = {
            let k = &key.public_key;
            const MAX: usize = 40;
            if k.len() > MAX {
                format!("{}...", &k[..MAX - 3])
            } else {
                k.clone()
            }
        };

        let auth_note = key_repo_map
            .get(&key.public_key)
            .map(|repos| format!(" (authorized: {})", repos.join(", ")))
            .unwrap_or_default();

        println!(
            "  {}  {}  {}{}",
            key.name.cyan().bold(),
            key.key_type.to_string().dimmed(),
            short_key.dimmed(),
            auth_note.green()
        );
    }

    Ok(())
}

fn handle_rename(old_name: &str, new_name: &str) -> Result<()> {
    validate_key_name(new_name)?;

    let dir = identities_dir()?;
    let old_path = dir.join(old_name);
    let new_path = dir.join(new_name);

    if !old_path.exists() {
        anyhow::bail!("Key '{}' not found in {}", old_name, dir.display());
    }

    if new_path.exists() {
        anyhow::bail!(
            "A key named '{}' already exists in {}",
            new_name,
            dir.display()
        );
    }

    fs::rename(&old_path, &new_path).with_context(|| {
        format!(
            "renaming key from '{}' to '{}'",
            old_path.display(),
            new_path.display()
        )
    })?;

    emit(
        Level::Success,
        "dot.key.rename.success",
        &format!(
            "{} Renamed key '{}' → '{}'",
            char::from(NerdFont::Check),
            old_name.cyan(),
            new_name.cyan()
        ),
        None,
    );

    Ok(())
}

/// Public wrapper for TUI to call handle_rename.
pub fn handle_rename_public(old_name: &str, new_name: &str) -> Result<()> {
    handle_rename(old_name, new_name)
}

/// Check which repos a given public key is authorized in.
pub(crate) fn find_repos_using_key(config: &DotfileConfig, public_key: &str) -> Vec<String> {
    let writable_repos = config.get_writable_repos();
    writable_repos
        .iter()
        .filter_map(|r| {
            let dotfile_repo = DotfileRepo::new(config, r.name.clone()).ok()?;
            let repo_path = dotfile_repo.local_path(config).ok()?;
            let meta = crate::dot::meta::read_meta(&repo_path).ok()?;
            if meta
                .encryption_recipients
                .iter()
                .any(|rec| rec == public_key)
            {
                Some(r.name.clone())
            } else {
                None
            }
        })
        .collect()
}

fn handle_remove(config: &DotfileConfig, name: &str) -> Result<()> {
    let dir = identities_dir()?;
    let key_path = dir.join(name);

    if !key_path.exists() {
        anyhow::bail!("Key '{}' not found in {}", name, dir.display());
    }

    // Read public key to check repo usage
    let mut public_key_opt = None;
    if let Ok(content) = fs::read_to_string(&key_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("AGE-SECRET-KEY-1")
                && let Ok(identity) = age::x25519::Identity::from_str(trimmed)
            {
                public_key_opt = Some(identity.to_public().to_string());
                break;
            }
        }
    }

    if let Some(pk) = &public_key_opt {
        let repos_using_key = find_repos_using_key(config, pk);

        if !repos_using_key.is_empty() {
            emit(
                Level::Warn,
                "dot.key.remove.in_use",
                &format!(
                    "{} Key '{}' is authorized in: {}.\n{} Removing it will prevent decryption of those repositories until a different key is authorized.",
                    char::from(NerdFont::Warning).to_string().yellow(),
                    name.cyan(),
                    repos_using_key.join(", ").yellow(),
                    char::from(NerdFont::Warning).to_string().yellow()
                ),
                None,
            );
        }
    }

    fs::remove_file(&key_path)
        .with_context(|| format!("Failed to delete key file at {}", key_path.display()))?;

    emit(
        Level::Success,
        "dot.key.remove.success",
        &format!(
            "{} Removed key '{}' ({})",
            char::from(NerdFont::Check),
            name.cyan(),
            key_path.display()
        ),
        None,
    );

    Ok(())
}

pub fn get_local_public_keys() -> Result<Vec<String>> {
    let files = crate::dot::encryption::discover_identity_files();
    let mut pubkeys = Vec::new();
    for path in files {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("reading identity file {}", path.display()))?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("AGE-SECRET-KEY-1")
                && let Ok(identity) = age::x25519::Identity::from_str(trimmed)
            {
                pubkeys.push(identity.to_public().to_string());
            }
        }
    }
    Ok(pubkeys)
}

fn find_age_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                find_age_files(&path, files)?;
            } else if path.is_file() && crate::dot::encryption::is_encrypted_source(&path) {
                files.push(path);
            }
        }
    }
    Ok(())
}

pub(crate) fn handle_identity() -> Result<()> {
    let keys = discover_all_keys_info()?;

    println!(
        "{} Local Machine Encryption Keys",
        char::from(NerdFont::Key).to_string().cyan()
    );
    println!(
        "{}",
        "────────────────────────────────────────────────────────────────────────────────".cyan()
    );

    if keys.is_empty() {
        println!(
            "  {} No local encryption keys or SSH keys found on this machine.",
            char::from(NerdFont::Warning).to_string().red()
        );
        println!(
            "  {} Run `ins dot keys generate` to generate a secure local encryption keypair.",
            char::from(NerdFont::Lightbulb).to_string().yellow()
        );
        println!();
        return Ok(());
    }

    for key in &keys {
        println!(
            "  {} ({})",
            key.name.cyan().bold(),
            key.key_type.to_string().dimmed()
        );
        println!("    {}", key.public_key.green());
        println!("    ({})", key.path.display().to_string().dimmed());
        println!();
    }

    println!(
        "  {} Share these public keys with others to allow them to authorize you as a recipient.",
        char::from(NerdFont::InfoCircle).to_string().dimmed()
    );
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_find_age_files_recursively() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let age_file1 = path.join("test.txt.age");
        let plain_file = path.join("test.txt");
        let sub = path.join("sub");
        fs::create_dir(&sub).unwrap();
        let age_file2 = sub.join("another.toml.age");

        fs::write(&age_file1, "").unwrap();
        fs::write(&plain_file, "").unwrap();
        fs::write(&age_file2, "").unwrap();

        let mut discovered = Vec::new();
        find_age_files(path, &mut discovered).unwrap();

        assert_eq!(discovered.len(), 2);
        assert!(discovered.contains(&age_file1));
        assert!(discovered.contains(&age_file2));
    }

    #[test]
    fn test_authorize_and_rotate_operation() {
        let temp = tempdir().unwrap();
        let repo_dir = temp.path().join("my-repo");
        fs::create_dir_all(repo_dir.join("dots")).unwrap();

        // 1. Generate encryption keys
        let id1 = age::x25519::Identity::generate();
        let pub1 = id1.to_public().to_string();

        let id2 = age::x25519::Identity::generate();
        let pub2 = id2.to_public().to_string();

        // 2. Initialize repo metadata and git
        crate::common::git::init_repo(&repo_dir).unwrap();
        fs::write(repo_dir.join("instantdots.toml"), "").unwrap();

        let meta = crate::dot::types::RepoMetaData {
            name: "my-repo".to_string(),
            dots_dirs: vec!["dots".to_string()],
            encryption_recipients: vec![pub1.clone()],
            ..Default::default()
        };
        crate::dot::meta::update_meta(&repo_dir, &meta).unwrap();

        // 3. Encrypt an initial file for id1
        let plain_bytes = b"super secret password";
        let parsed_recipients = crate::dot::encryption::parse_recipients(&[pub1.clone()]).unwrap();
        let cipher_bytes =
            crate::dot::encryption::encrypt_bytes_to_armored(plain_bytes, &parsed_recipients)
                .unwrap();
        let encrypted_file_path = repo_dir.join("dots/secrets.txt.age");
        fs::write(&encrypted_file_path, &cipher_bytes).unwrap();

        // 4. Setup mock DotfileConfig and DB
        let config_file = temp.path().join("dots.toml");
        fs::write(
            &config_file,
            format!(
                r#"
            clone_depth = 1
            [[repos]]
            url = "{}"
            name = "my-repo"
            enabled = true
            "#,
                repo_dir.to_string_lossy()
            ),
        )
        .unwrap();

        let mut config = DotfileConfig::load(Some(config_file.to_str().unwrap())).unwrap();
        config.repos_dir = crate::common::TildePath::new(temp.path().to_path_buf());
        let db_file = temp.path().join("instant.db");
        let db = Database::new(db_file).unwrap();

        // 5. Mock discover identities by setting env var
        let identity_file = temp.path().join("my_identity");
        use age::secrecy::ExposeSecret;
        let id1_string = id1.to_string();
        fs::write(&identity_file, id1_string.expose_secret()).unwrap();
        let age_guard = crate::dot::test_util::EnvGuard::set("AGE_IDENTITY", &identity_file);

        // 6. Test Authorize Operation
        handle_authorize(&config, &db, Some(&pub2), Some("my-repo"), false, false).unwrap();

        // Check metadata updated
        let updated_meta = crate::dot::meta::read_meta(&repo_dir).unwrap();
        assert!(updated_meta.encryption_recipients.contains(&pub1));
        assert!(updated_meta.encryption_recipients.contains(&pub2));

        // 7. Verify we can decrypt with the new key (id2)
        let newly_encrypted_bytes = fs::read(&encrypted_file_path).unwrap();
        let decryptor = age::Decryptor::new_buffered(age::armor::ArmoredReader::new(
            newly_encrypted_bytes.as_slice(),
        ))
        .unwrap();
        let mut reader = decryptor
            .decrypt(
                vec![Box::new(id2.clone()) as Box<dyn age::Identity>]
                    .iter()
                    .map(|i| i.as_ref() as &dyn age::Identity),
            )
            .unwrap();
        let mut decrypted_payload = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut decrypted_payload).unwrap();
        assert_eq!(decrypted_payload, plain_bytes);

        // 8. Setup id2 as the local key to test rotating out id1.
        //    Drop the first guard before installing the second so the
        //    second EnvGuard captures the original (pre-test) value of
        //    AGE_IDENTITY rather than the previous test value.
        let identity_file2 = temp.path().join("my_identity2");
        let id2_string = id2.to_string();
        fs::write(&identity_file2, id2_string.expose_secret()).unwrap();
        drop(age_guard);
        let _age_guard2 = crate::dot::test_util::EnvGuard::set("AGE_IDENTITY", &identity_file2);

        // 9. Test Rotate Operation (only allow id2)
        handle_rotate(&config, &db, &[pub2.clone()], Some("my-repo"), false, false).unwrap();

        let rotated_meta = crate::dot::meta::read_meta(&repo_dir).unwrap();
        assert_eq!(rotated_meta.encryption_recipients, vec![pub2.clone()]);
        // _age_guard2 restores AGE_IDENTITY on drop at end of scope.
    }
}
