use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        anyhow::bail!("{} does not exist", path.display());
    }
    path.canonicalize()
        .with_context(|| format!("Failed to canonicalize path {}", path.display()))
}

pub fn compute_file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read {} for hashing", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn extension_or_default(path: &Path, default: &str) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_string())
        .unwrap_or_else(|| default.to_string())
}
