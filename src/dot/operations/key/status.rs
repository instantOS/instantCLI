use anyhow::Result;
use colored::Colorize;

use crate::dot::config::DotfileConfig;
use crate::dot::dotfilerepo::DotfileRepo;
use crate::ui::prelude::*;

use super::discover::{discover_all_keys_info, get_local_public_keys};
use super::util::find_age_files;

/// Show key status per repository.
pub fn handle_status(config: &DotfileConfig, target_repo_opt: Option<&str>) -> Result<()> {
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

/// List all local encryption keys with authorization info.
pub fn handle_list(config: &DotfileConfig) -> Result<()> {
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

/// Show local identity keys (human-readable).
pub fn handle_identity() -> Result<()> {
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
