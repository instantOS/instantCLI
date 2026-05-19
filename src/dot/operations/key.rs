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
        EncryptCommands::Generate { force } => handle_init(*force),
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

pub(crate) fn handle_init(force: bool) -> Result<()> {
    let config_dir = crate::common::paths::instant_config_dir()?;
    let identity_dir = config_dir.join("encryption");
    let identity_path = identity_dir.join("identity");

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
                    "{} Encryption key already exists!\n{} Path: {}\n{} Public recipient key: {}",
                    char::from(NerdFont::Info),
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

    fs::create_dir_all(&identity_dir)?;
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
            "{} Generated secure encryption keypair for this machine!\n{} Private key saved to: {}\n{} Public key: {}",
            char::from(NerdFont::Check),
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
                    "     {} Hint: Place the matching private key in ~/.config/instant/encryption/identity",
                    char::from(NerdFont::Lightbulb).to_string().yellow()
                );
            }
        }
        println!();
    }

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
    let config_dir = crate::common::paths::instant_config_dir()?;
    let identity_dir = config_dir.join("encryption");
    let identity_path = identity_dir.join("identity");

    println!(
        "{} Local Machine Age Identity Public Keys",
        char::from(NerdFont::Key).to_string().cyan()
    );
    println!(
        "{}",
        "────────────────────────────────────────────────────────────────────────────────".cyan()
    );

    let mut identity_found = false;

    if identity_path.exists() {
        let content = fs::read_to_string(&identity_path)?;
        let mut pubkeys = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("AGE-SECRET-KEY-1")
                && let Ok(identity) = age::x25519::Identity::from_str(trimmed)
            {
                pubkeys.push(identity.to_public().to_string());
            }
        }
        if !pubkeys.is_empty() {
            identity_found = true;
            println!(
                "{} Encryption keys (from ~/.config/instant/encryption/identity):",
                char::from(NerdFont::CheckCircle).to_string().green()
            );
            for pk in pubkeys {
                println!("   {}", pk.green().bold());
            }
            println!(
                "   {} Share these public keys with others to allow them to authorize you as a recipient.",
                char::from(NerdFont::InfoCircle).to_string().dimmed()
            );
            println!();
        }
    }

    // Discover SSH public keys which can be used natively as age recipients!
    let mut ssh_keys = Vec::new();
    let home = std::env::var("HOME").map(PathBuf::from).ok();
    if let Some(home_path) = home {
        let ssh_dir = home_path.join(".ssh");
        if ssh_dir.is_dir()
            && let Ok(entries) = fs::read_dir(ssh_dir)
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
                        ssh_keys.push((
                            path.file_name().unwrap().to_string_lossy().into_owned(),
                            content_trimmed.to_string(),
                        ));
                    }
                }
            }
        }
    }

    if !ssh_keys.is_empty() {
        println!(
            "{} Discovered SSH Public Keys (natively supported for encryption):",
            char::from(NerdFont::Terminal).to_string().blue()
        );
        for (name, key) in &ssh_keys {
            println!(
                "   {} (from ~/.ssh/{}):",
                char::from(NerdFont::Bullet).to_string().blue(),
                name
            );
            println!("   {}", key.cyan());
        }
        println!();
    }

    if !identity_found && ssh_keys.is_empty() {
        println!(
            "  {} No local encryption keys or SSH keys found on this machine.",
            char::from(NerdFont::Warning).to_string().red()
        );
        println!(
            "  {} Run `ins dot keys generate` to generate a secure local encryption keypair.",
            char::from(NerdFont::Lightbulb).to_string().yellow()
        );
        println!();
    }

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
