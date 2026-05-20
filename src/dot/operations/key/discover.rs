use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use super::types::{KeyInfo, KeyType};

/// Discover all local identity keys (age + SSH) with full metadata.
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

/// Extract just the public key strings from local identity files.
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
