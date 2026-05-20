use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// Get the identities directory path, creating it if needed.
pub fn identities_dir() -> Result<PathBuf> {
    let config_dir = crate::common::paths::instant_config_dir()?;
    let dir = config_dir.join("encryption").join("identities");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Validate a key name: non-empty, no path separators, no `.`/`..`.
pub fn validate_key_name(name: &str) -> Result<()> {
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

/// Recursively find `.age` files under a directory.
pub fn find_age_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
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
