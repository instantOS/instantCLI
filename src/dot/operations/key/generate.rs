use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::str::FromStr;

use super::util::{identities_dir, validate_key_name};
use crate::ui::prelude::*;

pub fn handle_init(name: Option<&str>, force: bool) -> Result<()> {
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
