use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use sha2::Digest;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use sudo::RunningAs;

const REPO_OWNER: &str = "instantOS";
const REPO_NAME: &str = "instantCLI";
const BIN_NAME: &str = "ins";
const GITHUB_API_URL: &str = "https://api.github.com/repos";

#[derive(Debug, Clone)]
struct InstallLocation {
    path: PathBuf,
    needs_sudo: bool,
    is_managed: bool,
}

/// Check if the binary is installed in a package manager location
fn is_package_managed_location(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    // Check for system package manager locations
    if path_str.starts_with("/usr/bin") {
        return true;
    }

    // Check for cargo location
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        if path.starts_with(cargo_bin) {
            return true;
        }
    }

    false
}

/// Check if a directory is writable
fn is_writable(path: &Path) -> bool {
    if !path.exists() {
        // Check if we can create it
        return path.parent().is_some_and(is_writable);
    }

    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o200 != 0)
        .unwrap_or(false)
        && nix::unistd::access(path, nix::unistd::AccessFlags::W_OK).is_ok()
}

/// Get the current installation location
fn get_install_location() -> Result<InstallLocation> {
    let current_exe = env::current_exe().context("Failed to get current executable path")?;

    let is_managed = is_package_managed_location(&current_exe);

    if let Some(parent) = current_exe.parent() {
        let needs_sudo = !is_writable(parent);
        Ok(InstallLocation {
            path: current_exe,
            needs_sudo,
            is_managed,
        })
    } else {
        Err(anyhow!(
            "Could not determine parent directory of executable"
        ))
    }
}

/// Detect system architecture
fn detect_target() -> Result<String> {
    let arch = env::consts::ARCH;
    match arch {
        "x86_64" => Ok("x86_64-unknown-linux-gnu".to_string()),
        "aarch64" => Ok("aarch64-unknown-linux-gnu".to_string()),
        _ => Err(anyhow!("Unsupported architecture: {}", arch)),
    }
}

#[derive(serde::Deserialize, Debug)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(serde::Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

/// Fetch the latest release from GitHub
async fn fetch_latest_release() -> Result<GitHubRelease> {
    let url = format!(
        "{}/{}/{}/releases/latest",
        GITHUB_API_URL, REPO_OWNER, REPO_NAME
    );

    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", BIN_NAME, env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to fetch release information")?;

    if !response.status().is_success() {
        return Err(anyhow!("GitHub API returned status: {}", response.status()));
    }

    response
        .json::<GitHubRelease>()
        .await
        .context("Failed to parse release information")
}

/// Compare version strings
fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse_version = |v: &str| {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect::<Vec<_>>()
    };

    let current_parts = parse_version(current);
    let latest_parts = parse_version(latest);

    for (c, l) in current_parts.iter().zip(latest_parts.iter()) {
        if l > c {
            return true;
        } else if l < c {
            return false;
        }
    }

    latest_parts.len() > current_parts.len()
}

/// Find the appropriate asset for the target architecture
fn find_asset_url(release: &GitHubRelease, target: &str) -> Result<(String, Option<String>)> {
    let archive_asset = release
        .assets
        .iter()
        .find(|a| {
            a.name.contains(target) && (a.name.ends_with(".tar.zst") || a.name.ends_with(".tgz"))
        })
        .ok_or_else(|| anyhow!("No prebuilt archive found for {}", target))?;

    let sha_asset = release
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sha256", archive_asset.name));

    Ok((
        archive_asset.browser_download_url.clone(),
        sha_asset.map(|a| a.browser_download_url.clone()),
    ))
}

/// Download a file from URL
async fn download_file(url: &str, dest: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", BIN_NAME, env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to download file")?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let bytes = response.bytes().await.context("Failed to read response")?;
    fs::write(dest, bytes).context("Failed to write file")?;

    Ok(())
}

/// Verify checksum of downloaded file
async fn verify_checksum(archive_path: &Path, sha_url: Option<&str>) -> Result<()> {
    let Some(sha_url) = sha_url else {
        emit(
            Level::Warn,
            "self_update.checksum.skip",
            &format!(
                "{} No checksum available; skipping verification",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        return Ok(());
    };

    let checksum_file = archive_path.with_extension("sha256");
    download_file(sha_url, &checksum_file).await?;

    let checksum_content =
        fs::read_to_string(&checksum_file).context("Failed to read checksum file")?;

    let expected_hash = checksum_content
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("Invalid checksum file format"))?;

    let archive_bytes =
        fs::read(archive_path).context("Failed to read archive for verification")?;

    let actual_hash = format!("{:x}", sha2::Sha256::digest(archive_bytes));

    if actual_hash != expected_hash {
        return Err(anyhow!("Checksum verification failed"));
    }

    emit(
        Level::Success,
        "self_update.checksum.verified",
        &format!("{} Checksum verified", char::from(NerdFont::Check)),
        None,
    );
    Ok(())
}

/// Extract the archive
fn extract_archive(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    use std::process::Command;

    let filename = archive_path.to_string_lossy();

    if filename.ends_with(".tar.zst") {
        let output = Command::new("tar")
            .arg("--zstd")
            .arg("-xf")
            .arg(archive_path)
            .arg("-C")
            .arg(dest_dir)
            .output()
            .context("Failed to extract .tar.zst archive")?;

        if !output.status.success() {
            return Err(anyhow!(
                "tar extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    } else if filename.ends_with(".tgz") || filename.ends_with(".tar.gz") {
        let output = Command::new("tar")
            .arg("-xzf")
            .arg(archive_path)
            .arg("-C")
            .arg(dest_dir)
            .output()
            .context("Failed to extract .tgz archive")?;

        if !output.status.success() {
            return Err(anyhow!(
                "tar extraction failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    } else {
        return Err(anyhow!("Unsupported archive format"));
    }

    Ok(())
}

/// Find the binary in the extracted directory
fn find_binary_in_dir(search_dir: &Path, bin_name: &str) -> Result<PathBuf> {
    for entry in walkdir::WalkDir::new(search_dir) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.file_name() == bin_name {
            return Ok(entry.path().to_path_buf());
        }
    }
    Err(anyhow!(
        "Binary {} not found in extracted archive",
        bin_name
    ))
}

/// Install the binary (with or without sudo)
fn install_binary(binary_path: &Path, target_path: &Path, needs_sudo: bool) -> Result<()> {
    use std::process::Command;

    if needs_sudo {
        emit(
            Level::Info,
            "self_update.install.sudo",
            &format!(
                "{} Requesting elevated permissions to install...",
                char::from(NerdFont::Lock)
            ),
            None,
        );

        let status = Command::new("sudo")
            .arg("install")
            .arg("-m")
            .arg("755")
            .arg(binary_path)
            .arg(target_path)
            .status()
            .context("Failed to execute sudo install")?;

        if !status.success() {
            return Err(anyhow!("sudo install failed"));
        }
    } else {
        fs::copy(binary_path, target_path).context("Failed to copy binary")?;

        let mut perms = fs::metadata(target_path)
            .context("Failed to get binary permissions")?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(target_path, perms).context("Failed to set binary permissions")?;
    }

    Ok(())
}

/// Main self-update logic
pub async fn self_update() -> Result<()> {
    emit(
        Level::Info,
        "self_update.checking",
        &format!("{} Checking for updates...", char::from(NerdFont::Search)),
        None,
    );

    let location = get_install_location()?;

    // Check if installed via package manager
    if location.is_managed {
        emit(
            Level::Warn,
            "self_update.package_managed",
            &format!(
                "{} This installation appears to be managed by a package manager.",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        println!("   Please use your package manager to update instead.");

        if location.path.to_string_lossy().contains(".cargo/bin") {
            println!("   {}: cargo install ins", "Example".bright_black());
        } else {
            println!(
                "   {}: sudo pacman -S ins  (or your package manager)",
                "Example".bright_black()
            );
        }

        return Ok(());
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let target = detect_target()?;
    let release = fetch_latest_release().await?;

    let latest_version = release.tag_name.trim_start_matches('v');

    emit(
        Level::Info,
        "self_update.version.current",
        &format!(
            "{} Current version: {}",
            char::from(NerdFont::Info),
            current_version
        ),
        None,
    );
    emit(
        Level::Info,
        "self_update.version.latest",
        &format!(
            "{} Latest version: {}",
            char::from(NerdFont::Info),
            latest_version
        ),
        None,
    );

    if !is_newer_version(current_version, latest_version) {
        emit(
            Level::Success,
            "self_update.up_to_date",
            &format!("{} Already up to date!", char::from(NerdFont::Check)),
            None,
        );
        return Ok(());
    }

    emit(
        Level::Success,
        "self_update.available",
        &format!(
            "{} Update available: {} → {}",
            char::from(NerdFont::Upgrade),
            current_version,
            latest_version
        ),
        None,
    );

    // Confirm update
    use dialoguer::Confirm;
    if !Confirm::new()
        .with_prompt("Do you want to update?")
        .default(true)
        .interact()?
    {
        emit(
            Level::Info,
            "self_update.cancelled",
            &format!("{} Update cancelled", char::from(NerdFont::Info)),
            None,
        );
        return Ok(());
    }

    let (archive_url, sha_url) = find_asset_url(&release, &target)?;

    // Create temporary directory
    let temp_dir = tempfile::tempdir().context("Failed to create temporary directory")?;
    let archive_name = archive_url.split('/').next_back().unwrap_or("archive");
    let archive_path = temp_dir.path().join(archive_name);

    emit(
        Level::Info,
        "self_update.downloading",
        &format!("{} Downloading update...", char::from(NerdFont::Download)),
        None,
    );
    download_file(&archive_url, &archive_path).await?;

    verify_checksum(&archive_path, sha_url.as_deref()).await?;

    emit(
        Level::Info,
        "self_update.extracting",
        &format!("{} Extracting archive...", char::from(NerdFont::Archive)),
        None,
    );
    let extract_dir = temp_dir.path().join("extracted");
    fs::create_dir(&extract_dir)?;
    extract_archive(&archive_path, &extract_dir)?;

    let new_binary = find_binary_in_dir(&extract_dir, BIN_NAME)?;

    emit(
        Level::Info,
        "self_update.installing",
        &format!("{} Installing update...", char::from(NerdFont::Gear)),
        None,
    );

    // Handle privilege escalation if needed
    if location.needs_sudo {
        match sudo::check() {
            RunningAs::Root | RunningAs::Suid => {
                // Already running with elevated privileges, proceed
                install_binary(&new_binary, &location.path, false)?;
            }
            RunningAs::User => {
                // Need sudo
                install_binary(&new_binary, &location.path, true)?;
            }
        }
    } else {
        install_binary(&new_binary, &location.path, false)?;
    }

    emit(
        Level::Success,
        "self_update.complete",
        &format!(
            "{} Successfully updated: {} → {}",
            char::from(NerdFont::Check),
            current_version,
            latest_version
        ),
        None,
    );

    Ok(())
}
