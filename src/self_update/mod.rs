use crate::common::distro::OperatingSystem;
use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::Digest;
use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use sudo::RunningAs;

const REPO_OWNER: &str = "instantOS";
const REPO_NAME: &str = "instantCLI";
const BIN_NAME: &str = "ins";
const CRATE_NAME: &str = "ins";
const GITHUB_API_URL: &str = "https://api.github.com/repos";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallSource {
    Cargo,
    SystemPackage,
    Standalone,
}

#[derive(Debug, Clone)]
struct InstallLocation {
    path: PathBuf,
    needs_sudo: bool,
    install_source: InstallSource,
}

fn cargo_bin_dir() -> Option<PathBuf> {
    if let Ok(cargo_home) = env::var("CARGO_HOME")
        && !cargo_home.trim().is_empty()
    {
        return Some(PathBuf::from(cargo_home).join("bin"));
    }

    dirs::home_dir().map(|home| home.join(".cargo").join("bin"))
}

/// Detect where the binary was installed from
fn detect_install_source(path: &Path) -> InstallSource {
    // Check for system package manager locations
    if path.starts_with("/usr/bin") {
        return InstallSource::SystemPackage;
    }

    // Check for cargo location
    if let Some(cargo_bin) = cargo_bin_dir()
        && path.starts_with(cargo_bin)
    {
        return InstallSource::Cargo;
    }

    InstallSource::Standalone
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
    let install_source = detect_install_source(&current_exe);

    if let Some(parent) = current_exe.parent() {
        let needs_sudo = !is_writable(parent);
        Ok(InstallLocation {
            path: current_exe,
            needs_sudo,
            install_source,
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

    // Check for Termux environment
    if env::var("TERMUX_VERSION").is_ok() && arch == "aarch64" {
        return Ok("aarch64-termux".to_string());
    }

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

#[derive(serde::Deserialize, Debug)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
}

#[derive(serde::Deserialize, Debug)]
struct CrateInfo {
    max_version: String,
}

/// Fetch the latest release from GitHub that has binaries available
async fn fetch_latest_release_with_binaries(
    target: &str,
    is_steamos: bool,
) -> Result<GitHubRelease> {
    let url = format!("{}/{}/{}/releases", GITHUB_API_URL, REPO_OWNER, REPO_NAME);

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

    let releases: Vec<GitHubRelease> = response
        .json()
        .await
        .context("Failed to parse release information")?;

    // Find the first release that has the required binary for this target
    for release in releases {
        let has_binary = release.assets.iter().any(|a| {
            if is_steamos {
                a.name.ends_with(".AppImage") && !a.name.contains(".sha256")
            } else {
                a.name.contains(target)
                    && (a.name.ends_with(".tar.zst") || a.name.ends_with(".tgz"))
            }
        });

        if has_binary {
            return Ok(release);
        }
    }

    Err(anyhow!(
        "No release found with binaries for target: {}{}",
        target,
        if is_steamos { " (AppImage)" } else { "" }
    ))
}

/// Fetch the latest published crate version from crates.io
async fn fetch_latest_crate_version(crate_name: &str) -> Result<String> {
    let url = format!("https://crates.io/api/v1/crates/{}", crate_name);

    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", BIN_NAME, env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to fetch crate information")?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "crates.io API returned status: {}",
            response.status()
        ));
    }

    let crate_info: CratesIoResponse = response
        .json()
        .await
        .context("Failed to parse crate information")?;

    Ok(crate_info.krate.max_version)
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

fn confirm_update() -> Result<bool> {
    use dialoguer::Confirm;

    Ok(Confirm::new()
        .with_prompt("Do you want to update?")
        .default(true)
        .interact()?)
}

/// Find the appropriate asset for the target architecture
fn find_asset_url(
    release: &GitHubRelease,
    target: &str,
    is_steamos: bool,
) -> Result<(String, Option<String>)> {
    let archive_asset = if is_steamos {
        release
            .assets
            .iter()
            .find(|a| a.name.ends_with(".AppImage") && !a.name.contains(".sha256"))
            .ok_or_else(|| anyhow!("No AppImage found for SteamOS"))?
    } else {
        release
            .assets
            .iter()
            .find(|a| {
                a.name.contains(target)
                    && (a.name.ends_with(".tar.zst") || a.name.ends_with(".tgz"))
            })
            .ok_or_else(|| anyhow!("No prebuilt archive found for {}", target))?
    };

    let sha_asset = release
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sha256", archive_asset.name));

    Ok((
        archive_asset.browser_download_url.clone(),
        sha_asset.map(|a| a.browser_download_url.clone()),
    ))
}

/// Download a file from URL with progress bar
async fn download_file(url: &str, dest: &Path, show_progress: bool) -> Result<()> {
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

    let total_size = response.content_length();

    let pb = if show_progress {
        let pb = if let Some(size) = total_size {
            ProgressBar::new(size)
        } else {
            ProgressBar::new_spinner()
        };

        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    let mut file = fs::File::create(dest).context("Failed to create destination file")?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk: bytes::Bytes = item.context("Error while downloading file")?;
        file.write_all(&chunk)
            .context("Error while writing to file")?;
        let new = downloaded + (chunk.len() as u64);
        downloaded = new;
        if let Some(ref pb) = pb {
            pb.set_position(new);
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Download complete");
    }

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
    download_file(sha_url, &checksum_file, false).await?;

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
/// Uses a two-phase approach to avoid "Text file busy" error
fn install_binary(binary_path: &Path, target_path: &Path, needs_sudo: bool) -> Result<()> {
    use std::process::Command;

    // Create a temporary path alongside the target for atomic replacement
    let temp_target = target_path.with_extension("new");

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

        // First, install to temporary location
        let status = Command::new("sudo")
            .arg("install")
            .arg("-m")
            .arg("755")
            .arg(binary_path)
            .arg(&temp_target)
            .status()
            .context("Failed to execute sudo install to temporary location")?;

        if !status.success() {
            return Err(anyhow!("sudo install failed"));
        }

        // Then atomically move it to the target location
        let status = Command::new("sudo")
            .arg("mv")
            .arg("-f")
            .arg(&temp_target)
            .arg(target_path)
            .status()
            .context("Failed to move binary to final location")?;

        if !status.success() {
            return Err(anyhow!("sudo mv failed"));
        }
    } else {
        // Copy to temporary location first
        fs::copy(binary_path, &temp_target)
            .context("Failed to copy binary to temporary location")?;

        let mut perms = fs::metadata(&temp_target)
            .context("Failed to get binary permissions")?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_target, perms).context("Failed to set binary permissions")?;

        // Atomically rename to final location (this works even on running executables)
        fs::rename(&temp_target, target_path).context("Failed to move binary to final location")?;
    }

    Ok(())
}

async fn self_update_via_cargo(current_version: &str) -> Result<()> {
    use std::io::ErrorKind;
    use std::process::Command;

    emit(
        Level::Info,
        "self_update.cargo.detected",
        &format!(
            "{} Detected Cargo installation; checking crates.io...",
            char::from(NerdFont::Package)
        ),
        None,
    );

    let latest_version = fetch_latest_crate_version(CRATE_NAME).await?;

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

    if !is_newer_version(current_version, &latest_version) {
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

    if !confirm_update()? {
        emit(
            Level::Info,
            "self_update.cancelled",
            &format!("{} Update cancelled", char::from(NerdFont::Info)),
            None,
        );
        return Ok(());
    }

    emit(
        Level::Info,
        "self_update.cargo.installing",
        &format!(
            "{} Running cargo install --force {}...",
            char::from(NerdFont::Package),
            CRATE_NAME
        ),
        None,
    );

    let status = Command::new("cargo")
        .arg("install")
        .arg("--force")
        .arg(CRATE_NAME)
        .status();

    match status {
        Ok(status) => {
            if !status.success() {
                return Err(anyhow!("cargo install failed"));
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            emit(
                Level::Warn,
                "self_update.cargo.missing",
                &format!(
                    "{} Cargo was not found in PATH; unable to update automatically.",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            println!(
                "   {}: cargo install --force {}",
                "Example".bright_black(),
                CRATE_NAME
            );
            return Ok(());
        }
        Err(err) => return Err(err.into()),
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

/// Main self-update logic
pub async fn self_update() -> Result<()> {
    emit(
        Level::Info,
        "self_update.checking",
        &format!("{} Checking for updates...", char::from(NerdFont::Search)),
        None,
    );

    let current_version = env!("CARGO_PKG_VERSION");
    let mut location = get_install_location()?;
    let is_steamos = OperatingSystem::detect() == OperatingSystem::SteamOS;

    if is_steamos {
        let home = dirs::home_dir().context("Could not find home directory")?;
        let local_bin = home.join(".local").join("bin");
        location.path = local_bin.join(BIN_NAME);
        location.needs_sudo = false;
        location.install_source = InstallSource::Standalone;

        if !local_bin.exists() {
            fs::create_dir_all(&local_bin).context("Failed to create ~/.local/bin")?;
        }
    }

    match location.install_source {
        InstallSource::SystemPackage => {
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
            println!(
                "   {}: sudo <package-manager> install ins",
                "Example".bright_black()
            );
            return Ok(());
        }
        InstallSource::Cargo => {
            self_update_via_cargo(current_version).await?;
            return Ok(());
        }
        InstallSource::Standalone => {}
    }

    let target = detect_target()?;
    let release = fetch_latest_release_with_binaries(&target, is_steamos).await?;

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

    if !confirm_update()? {
        emit(
            Level::Info,
            "self_update.cancelled",
            &format!("{} Update cancelled", char::from(NerdFont::Info)),
            None,
        );
        return Ok(());
    }

    let (archive_url, sha_url) = find_asset_url(&release, &target, is_steamos)?;

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
    download_file(&archive_url, &archive_path, true).await?;

    verify_checksum(&archive_path, sha_url.as_deref()).await?;

    let new_binary = if is_steamos {
        // AppImage is the binary itself
        let mut perms = fs::metadata(&archive_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&archive_path, perms)?;
        archive_path
    } else {
        emit(
            Level::Info,
            "self_update.extracting",
            &format!("{} Extracting archive...", char::from(NerdFont::Archive)),
            None,
        );
        let extract_dir = temp_dir.path().join("extracted");
        fs::create_dir(&extract_dir)?;
        extract_archive(&archive_path, &extract_dir)?;
        find_binary_in_dir(&extract_dir, BIN_NAME)?
    };

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
