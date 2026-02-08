use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    let file_size = file
        .metadata()
        .with_context(|| format!("Failed to read metadata for {}", path.display()))?
        .len();
    let mut hasher = Sha256::new();
    hasher.update(file_size.to_le_bytes());

    const SAMPLE_SIZE: usize = 64 * 1024;
    const MIN_SAMPLES: u64 = 8;
    const MAX_SAMPLES: u64 = 512;
    const TARGET_STEP: u64 = 8 * 1024 * 1024;
    let full_read_threshold = (SAMPLE_SIZE as u64) * MAX_SAMPLES;

    if file_size <= full_read_threshold {
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
    } else {
        let mut sample_count = file_size / TARGET_STEP;
        sample_count = sample_count.clamp(MIN_SAMPLES, MAX_SAMPLES);

        let mut buffer = vec![0u8; SAMPLE_SIZE];
        let last_offset = file_size.saturating_sub(SAMPLE_SIZE as u64);
        let step = if sample_count > 1 {
            last_offset / (sample_count - 1)
        } else {
            0
        };

        for i in 0..sample_count {
            let offset = step * i;
            file.seek(SeekFrom::Start(offset))
                .with_context(|| format!("Failed to seek {} for hashing", path.display()))?;
            let mut read_total = 0;
            while read_total < SAMPLE_SIZE {
                let read = file
                    .read(&mut buffer[read_total..])
                    .with_context(|| format!("Failed to read {} for hashing", path.display()))?;
                if read == 0 {
                    break;
                }
                read_total += read;
            }
            hasher.update(&buffer[..read_total]);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn extension_or_default(path: &Path, default: &str) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_string())
        .unwrap_or_else(|| default.to_string())
}

pub fn duration_to_tenths(duration: Duration) -> u64 {
    ((duration.as_secs_f64() * 10.0).round()) as u64
}
