use anyhow::Result;
use once_cell::sync::Lazy;
use semver::Version;
use std::process::Command;
use std::sync::atomic::AtomicBool;

pub static FZF_VERSION: Lazy<Option<Version>> = Lazy::new(|| get_fzf_version().ok());

pub static USE_LEGACY_ARGS: AtomicBool = AtomicBool::new(false);

fn get_fzf_version() -> Result<Version> {
    let output = Command::new("fzf").arg("--version").output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!("Failed to get fzf version"));
    }
    let version_str = String::from_utf8_lossy(&output.stdout);
    // Expected output is "0.44.1 (debian)" or "0.44.1"
    let version_part = version_str.split(' ').next().unwrap_or("").trim();
    Version::parse(version_part).map_err(|e| anyhow::anyhow!("Failed to parse fzf version: {}", e))
}
