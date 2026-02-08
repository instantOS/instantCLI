use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use crate::common::distro::OperatingSystem;
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

#[derive(Default)]
pub struct PacmanCacheCheck;

impl PacmanCacheCheck {
    const CACHE_DIR: &'static str = "/var/cache/pacman/pkg";
    const THRESHOLD_GB: u64 = 10;
    const THRESHOLD_BYTES: u64 = 10 * 1024 * 1024 * 1024; // 10 GB
}

#[async_trait]
impl DoctorCheck for PacmanCacheCheck {
    fn name(&self) -> &'static str {
        "Pacman Cache Size"
    }

    fn id(&self) -> &'static str {
        "pacman-cache"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any // Can read cache dir as any user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // Cleaning cache requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Only run on Arch-based systems
        let os = crate::common::distro::OperatingSystem::detect();
        if !os.in_family(&OperatingSystem::Arch) {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
        }

        // Skip on immutable OSes (no writable package cache)
        if os.is_immutable() {
            return CheckStatus::Skipped("Immutable OS (no writable package cache)".to_string());
        }

        // Calculate total size of pacman cache directory
        match calculate_dir_size(Self::CACHE_DIR).await {
            Ok(size) => {
                let size_gb = size as f64 / (1024.0 * 1024.0 * 1024.0);
                if size < Self::THRESHOLD_BYTES {
                    CheckStatus::Pass(format!(
                        "Pacman cache size: {:.2} GB (below {} GB threshold)",
                        size_gb,
                        Self::THRESHOLD_GB
                    ))
                } else {
                    CheckStatus::Warning {
                        message: format!(
                            "Pacman cache size: {:.2} GB (exceeds {} GB threshold)",
                            size_gb,
                            Self::THRESHOLD_GB
                        ),
                        fixable: true,
                    }
                }
            }
            Err(e) => CheckStatus::Fail {
                message: format!("Could not calculate cache size: {}", e),
                fixable: false,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Clean pacman cache using paccache (keeps last 3 versions)".to_string())
    }

    async fn fix(&self) -> Result<()> {
        // Use paccache to clean old packages, keeping last 3 versions
        let status = TokioCommand::new("paccache").arg("-r").status().await?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("paccache failed to run"))
        }
    }
}

#[derive(Default)]
pub struct PacmanStaleDownloadsCheck;

impl PacmanStaleDownloadsCheck {
    const CACHE_DIR: &'static str = "/var/cache/pacman/pkg";
}

#[async_trait]
impl DoctorCheck for PacmanStaleDownloadsCheck {
    fn name(&self) -> &'static str {
        "Pacman Stale Downloads"
    }

    fn id(&self) -> &'static str {
        "pacman-stale-downloads"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any // Can list directory as any user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // Removing directories requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Only run on Arch-based systems
        let os = crate::common::distro::OperatingSystem::detect();
        if !os.in_family(&OperatingSystem::Arch) {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
        }

        // Skip on immutable OSes (no writable package cache)
        if os.is_immutable() {
            return CheckStatus::Skipped("Immutable OS (no writable package cache)".to_string());
        }

        // Check if pacman is currently running - if so, download dirs are in use
        if is_pacman_running().await {
            return CheckStatus::Pass(
                "Pacman is running, download directories are in use".to_string(),
            );
        }

        // Look for download-* directories in the pacman cache
        // These are leftover from interrupted package downloads
        let entries = match std::fs::read_dir(Self::CACHE_DIR) {
            Ok(entries) => entries,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Could not read cache directory: {}", e),
                    fixable: false,
                };
            }
        };

        let stale_dirs: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let file_name = path.file_name()?.to_string_lossy().to_string();
                if path.is_dir() && file_name.starts_with("download-") {
                    Some(file_name)
                } else {
                    None
                }
            })
            .collect();

        if stale_dirs.is_empty() {
            CheckStatus::Pass("No stale download directories found".to_string())
        } else {
            CheckStatus::Warning {
                message: format!(
                    "Found {} stale download director{}: {}",
                    stale_dirs.len(),
                    if stale_dirs.len() == 1 { "y" } else { "ies" },
                    stale_dirs.join(", ")
                ),
                fixable: true,
            }
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Remove stale download directories from pacman cache".to_string())
    }

    async fn fix(&self) -> Result<()> {
        // Don't remove directories if pacman is running
        if is_pacman_running().await {
            return Err(anyhow::anyhow!(
                "Pacman is currently running, cannot remove download directories"
            ));
        }

        let entries = std::fs::read_dir(Self::CACHE_DIR)?;

        let mut removed = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(file_name) = path.file_name()
                && path.is_dir()
                && file_name.to_string_lossy().starts_with("download-")
            {
                std::fs::remove_dir_all(&path)?;
                removed += 1;
                println!("Removed: {}", path.display());
            }
        }

        println!(
            "Removed {} stale download director{}.",
            removed,
            if removed == 1 { "y" } else { "ies" }
        );
        Ok(())
    }
}

#[derive(Default)]
pub struct SmartHealthCheck;

#[async_trait]
impl DoctorCheck for SmartHealthCheck {
    fn name(&self) -> &'static str {
        "S.M.A.R.T. Disk Health"
    }

    fn id(&self) -> &'static str {
        "smart-health"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // smartctl requires root
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root
    }

    async fn execute(&self) -> CheckStatus {
        use crate::common::deps::SMARTMONTOOLS;
        use crate::common::systemd::{ServiceScope, SystemdManager};

        // First, check if smartctl is available
        if !SMARTMONTOOLS.is_installed() {
            return CheckStatus::Warning {
                message: "smartmontools not installed".to_string(),
                fixable: true,
            };
        }

        // Check if smartd service is enabled
        let manager = SystemdManager::new(ServiceScope::System);
        if !manager.is_enabled("smartd") {
            return CheckStatus::Warning {
                message: "smartd service not enabled for continuous monitoring".to_string(),
                fixable: true,
            };
        }

        // Scan for drives using smartctl --scan
        let scan_output = TokioCommand::new("smartctl").arg("--scan").output().await;

        let drives: Vec<String> = match scan_output {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|line| {
                        // Format: /dev/sda -d sat # ...
                        line.split_whitespace().next().map(|s| s.to_string())
                    })
                    .collect()
            }
            _ => {
                return CheckStatus::Warning {
                    message: "Could not scan for drives".to_string(),
                    fixable: false,
                };
            }
        };

        if drives.is_empty() {
            return CheckStatus::Pass("No S.M.A.R.T. capable drives detected".to_string());
        }

        let mut healthy_drives = Vec::new();
        let mut unhealthy_drives = Vec::new();
        let mut unsupported_drives = Vec::new();

        for drive in &drives {
            // Check health status with smartctl -H
            let health_output = TokioCommand::new("smartctl")
                .arg("-H")
                .arg(drive)
                .output()
                .await;

            match health_output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.contains("PASSED") || stdout.contains("OK") {
                        healthy_drives.push(drive.clone());
                    } else if stdout.contains("FAILED") {
                        unhealthy_drives.push(drive.clone());
                    } else if stdout.contains("Not supported")
                        || stdout.contains("Unknown USB bridge")
                    {
                        unsupported_drives.push(drive.clone());
                    } else {
                        // Treat unknown status as healthy if exit code is 0
                        if output.status.success() {
                            healthy_drives.push(drive.clone());
                        } else {
                            unsupported_drives.push(drive.clone());
                        }
                    }
                }
                Err(_) => {
                    unsupported_drives.push(drive.clone());
                }
            }
        }

        if !unhealthy_drives.is_empty() {
            return CheckStatus::Fail {
                message: format!(
                    "S.M.A.R.T. health FAILED on: {}",
                    unhealthy_drives.join(", ")
                ),
                fixable: false, // Can't fix failing hardware
            };
        }

        if healthy_drives.is_empty() && !unsupported_drives.is_empty() {
            return CheckStatus::Pass(format!(
                "{} drive(s) do not support S.M.A.R.T.",
                unsupported_drives.len()
            ));
        }

        CheckStatus::Pass(format!("All {} drive(s) healthy", healthy_drives.len()))
    }

    fn fix_message(&self) -> Option<String> {
        Some("Install smartmontools and enable smartd service".to_string())
    }

    async fn fix(&self) -> Result<()> {
        use crate::common::deps::SMARTMONTOOLS;
        use crate::common::package::{InstallResult, ensure_all};
        use crate::common::systemd::{ServiceScope, SystemdManager};

        // Install smartmontools using the standard ensure_all() flow if not installed
        if !SMARTMONTOOLS.is_installed() {
            match ensure_all(&[&SMARTMONTOOLS])? {
                InstallResult::Installed | InstallResult::AlreadyInstalled => {}
                _ => return Err(anyhow::anyhow!("smartmontools installation cancelled")),
            }
        }

        // Enable and start smartd service
        let manager = SystemdManager::new(ServiceScope::System);
        manager.enable_and_start("smartd")?;

        println!("smartd service enabled and started for continuous disk monitoring.");
        Ok(())
    }
}

#[derive(Default)]
pub struct PacmanDbSyncCheck;

impl PacmanDbSyncCheck {
    const SYNC_DIR: &'static str = "/var/lib/pacman/sync";
    const WARN_THRESHOLD_DAYS: u64 = 14;
}

#[async_trait]
impl DoctorCheck for PacmanDbSyncCheck {
    fn name(&self) -> &'static str {
        "Pacman Database Sync"
    }

    fn id(&self) -> &'static str {
        "pacman-db-sync"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any // Can read sync dir as any user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // pacman -Sy requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Only run on Arch-based systems
        let os = crate::common::distro::OperatingSystem::detect();
        if !os.in_family(&OperatingSystem::Arch) {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
        }

        // Skip on immutable OSes (no writable package cache)
        if os.is_immutable() {
            return CheckStatus::Skipped("Immutable OS (no writable package cache)".to_string());
        }

        use std::time::{Duration, SystemTime};

        // Find the most recent sync database file
        let sync_dir = std::path::Path::new(Self::SYNC_DIR);

        if !sync_dir.exists() {
            return CheckStatus::Fail {
                message: "Pacman sync directory not found".to_string(),
                fixable: true,
            };
        }

        let mut most_recent: Option<SystemTime> = None;

        // Read directory entries
        let entries = match std::fs::read_dir(sync_dir) {
            Ok(entries) => entries,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Could not read sync directory: {}", e),
                    fixable: false,
                };
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("db")
                && let Ok(metadata) = path.metadata()
                && let Ok(modified) = metadata.modified()
            {
                most_recent = Some(match most_recent {
                    Some(current) => current.max(modified),
                    None => modified,
                });
            }
        }

        let last_sync = match most_recent {
            Some(time) => time,
            None => {
                return CheckStatus::Fail {
                    message: "No pacman database files found".to_string(),
                    fixable: true,
                };
            }
        };

        // Calculate age
        let age = SystemTime::now()
            .duration_since(last_sync)
            .unwrap_or(Duration::ZERO);

        let age_days = age.as_secs() / (60 * 60 * 24);
        let age_hours = (age.as_secs() % (60 * 60 * 24)) / (60 * 60);

        let age_str = if age_days > 0 {
            format!("{} day(s), {} hour(s) ago", age_days, age_hours)
        } else {
            format!("{} hour(s) ago", age_hours)
        };

        if age_days >= Self::WARN_THRESHOLD_DAYS {
            CheckStatus::Warning {
                message: format!(
                    "Database last synced {} (over {} days)",
                    age_str,
                    Self::WARN_THRESHOLD_DAYS
                ),
                fixable: true,
            }
        } else {
            CheckStatus::Pass(format!("Database last synced {}", age_str))
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Refresh pacman database with pacman -Sy".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let status = TokioCommand::new("pacman").arg("-Sy").status().await?;

        if status.success() {
            println!("Pacman database refreshed successfully.");
            Ok(())
        } else {
            Err(anyhow::anyhow!("pacman -Sy failed"))
        }
    }
}

#[derive(Default)]
pub struct YayCacheCheck;

impl YayCacheCheck {
    const THRESHOLD_GB: u64 = 10;
    const THRESHOLD_BYTES: u64 = 10 * 1024 * 1024 * 1024; // 10 GB

    fn get_cache_dir() -> Result<String> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        Ok(home_dir
            .join(".cache")
            .join("yay")
            .to_string_lossy()
            .to_string())
    }
}

#[async_trait]
impl DoctorCheck for YayCacheCheck {
    fn name(&self) -> &'static str {
        "Yay Cache Size"
    }

    fn id(&self) -> &'static str {
        "yay-cache"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User // Must run as user since this is in user's home directory
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User // Can be cleaned up as user
    }

    async fn execute(&self) -> CheckStatus {
        // Only run on Arch-based systems
        let os = crate::common::distro::OperatingSystem::detect();
        if !os.in_family(&OperatingSystem::Arch) {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
        }

        // Skip on immutable OSes (no writable package cache)
        if os.is_immutable() {
            return CheckStatus::Skipped("Immutable OS (no writable package cache)".to_string());
        }

        // Check if yay is installed
        let yay_installed = which::which("yay").is_ok();

        if !yay_installed {
            return CheckStatus::Skipped("Yay is not installed".to_string());
        }

        // Get cache directory path
        let cache_dir = match Self::get_cache_dir() {
            Ok(dir) => dir,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Could not determine cache directory: {}", e),
                    fixable: false,
                };
            }
        };

        let cache_path = std::path::Path::new(&cache_dir);

        // If cache directory doesn't exist, the check passes (no cache to worry about)
        if !cache_path.exists() {
            return CheckStatus::Pass("Yay cache directory does not exist".to_string());
        }

        // Calculate cache size
        let cache_size = match calculate_dir_size(&cache_dir).await {
            Ok(size) => size,
            Err(e) => {
                return CheckStatus::Fail {
                    message: format!("Could not calculate yay cache size: {}", e),
                    fixable: false,
                };
            }
        };

        if cache_size < Self::THRESHOLD_BYTES {
            CheckStatus::Pass(format!(
                "Yay cache is {:.2} GB (below {} GB threshold)",
                cache_size as f64 / 1024.0 / 1024.0 / 1024.0,
                Self::THRESHOLD_GB
            ))
        } else {
            CheckStatus::Warning {
                message: format!(
                    "Yay cache is {:.2} GB (exceeds {} GB threshold)",
                    cache_size as f64 / 1024.0 / 1024.0 / 1024.0,
                    Self::THRESHOLD_GB
                ),
                fixable: true,
            }
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Clear yay cache to free up disk space".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let cache_dir = Self::get_cache_dir()?;
        let cache_path = std::path::Path::new(&cache_dir);

        if cache_path.exists() {
            tokio::fs::remove_dir_all(cache_path)
                .await
                .context("Failed to remove yay cache directory")?;
            println!("Yay cache cleared successfully.");
        } else {
            println!("Yay cache directory does not exist, nothing to clear.");
        }

        Ok(())
    }
}

/// Helper function to calculate directory size (moved from main checks.rs)
async fn calculate_dir_size(path: &str) -> Result<u64> {
    let mut total_size: u64 = 0;
    let mut dirs_to_visit = vec![std::path::PathBuf::from(path)];

    while let Some(dir) = dirs_to_visit.pop() {
        // Skip directories we can't read (e.g., pacman temp download dirs with 700 permissions)
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(_) => continue, // Skip inaccessible directories
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            // Skip entries we can't stat
            let Ok(metadata) = entry.metadata().await else {
                continue;
            };
            if metadata.is_file() {
                total_size += metadata.len();
            } else if metadata.is_dir() {
                dirs_to_visit.push(entry.path());
            }
        }
    }

    Ok(total_size)
}

/// Check if pacman is currently running by looking for its lock file
async fn is_pacman_running() -> bool {
    // Pacman uses /var/lib/pacman/db.lck as a lock file when running
    std::path::Path::new("/var/lib/pacman/db.lck").exists()
}
