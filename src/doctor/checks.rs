use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::Result;
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

#[derive(Default)]
pub struct InternetCheck;

#[async_trait]
impl DoctorCheck for InternetCheck {
    fn name(&self) -> &'static str {
        "Internet Connectivity"
    }

    fn id(&self) -> &'static str {
        "internet"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User // nmtui should run as user
    }

    async fn execute(&self) -> CheckStatus {
        let output = TokioCommand::new("ping")
            .arg("-c")
            .arg("1")
            .arg("-W")
            .arg("1")
            .arg("8.8.8.8")
            .output()
            .await;

        match output {
            Ok(output) if output.status.success() => {
                CheckStatus::Pass("Internet connection is available".to_string())
            }
            _ => CheckStatus::Fail {
                message: "No internet connection detected".to_string(),
                fixable: true, // nmtui can potentially fix network issues
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Run nmtui to configure your network interface.".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let status = TokioCommand::new("nmtui").status().await?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("nmtui failed to run"))
        }
    }
}

#[derive(Default)]
pub struct InstantRepoCheck;

#[async_trait]
impl DoctorCheck for InstantRepoCheck {
    fn name(&self) -> &'static str {
        "InstantOS Repository Configuration"
    }

    fn id(&self) -> &'static str {
        "instant-repo"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any // Can read config as any user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // Modifying /etc/pacman.conf requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Only check on instantOS
        if !crate::common::distro::OperatingSystem::detect().is_instantos() {
            return CheckStatus::Skipped("Not running on instantOS".to_string());
        }

        // Check if /etc/pacman.conf contains [instant] section
        match tokio::fs::read_to_string("/etc/pacman.conf").await {
            Ok(content) => {
                if content.contains("[instant]")
                    && content.contains("/etc/pacman.d/instantmirrorlist")
                {
                    CheckStatus::Pass("InstantOS repository is configured".to_string())
                } else {
                    CheckStatus::Fail {
                        message: "InstantOS repository not found in pacman.conf".to_string(),
                        fixable: true, // We can add the repository configuration
                    }
                }
            }
            Err(_) => CheckStatus::Fail {
                message: "Could not read /etc/pacman.conf".to_string(),
                fixable: false, // If we can't read the file, we probably can't fix it either
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Add InstantOS repository configuration to /etc/pacman.conf".to_string())
    }

    async fn fix(&self) -> Result<()> {
        crate::common::pacman::setup_instant_repo(false).await
    }
}

#[derive(Default)]
pub struct LocaleCheck;

#[async_trait]
impl DoctorCheck for LocaleCheck {
    fn name(&self) -> &'static str {
        "UTF-8 Locale Configuration"
    }

    fn id(&self) -> &'static str {
        "locale"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // locale-gen requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Check LANG environment variable
        if let Ok(lang) = std::env::var("LANG") {
            // Check if it's not the C default and contains UTF-8
            let lang_upper = lang.to_uppercase();
            if lang_upper.contains("UTF-8") || lang_upper.contains("UTF8") {
                CheckStatus::Pass(format!("UTF-8 locale configured: {}", lang))
            } else if lang == "C" || lang == "POSIX" || lang.is_empty() {
                CheckStatus::Fail {
                    message: format!("Default C/POSIX locale detected: {}", lang),
                    fixable: true,
                }
            } else {
                CheckStatus::Fail {
                    message: format!("Non-UTF-8 locale configured: {}", lang),
                    fixable: true,
                }
            }
        } else {
            CheckStatus::Fail {
                message: "LANG environment variable not set".to_string(),
                fixable: true,
            }
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Generate and set a UTF-8 locale (e.g., en_US.UTF-8)".to_string())
    }

    async fn fix(&self) -> Result<()> {
        // Enable en_US.UTF-8 in locale.gen
        let locale_gen_path = "/etc/locale.gen";
        let content = tokio::fs::read_to_string(locale_gen_path).await?;

        // Check if en_US.UTF-8 is already uncommented
        let has_enabled_utf8 = content
            .lines()
            .any(|line| !line.starts_with('#') && line.contains("en_US.UTF-8"));

        if !has_enabled_utf8 {
            // Uncomment en_US.UTF-8 line or add it
            let new_content: String = if content.contains("#en_US.UTF-8") {
                content.replace("#en_US.UTF-8", "en_US.UTF-8")
            } else {
                format!("{}\nen_US.UTF-8 UTF-8\n", content)
            };
            tokio::fs::write(locale_gen_path, new_content).await?;
        }

        // Run locale-gen
        let status = TokioCommand::new("locale-gen").status().await?;
        if !status.success() {
            return Err(anyhow::anyhow!("locale-gen failed"));
        }

        // Set LANG in /etc/locale.conf
        tokio::fs::write("/etc/locale.conf", "LANG=en_US.UTF-8\n").await?;

        println!("Locale configured. Please log out and back in for changes to take effect.");
        Ok(())
    }
}

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
        if !crate::common::distro::OperatingSystem::detect().is_arch_based() {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
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
        if !crate::common::distro::OperatingSystem::detect().is_arch_based() {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
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
pub struct SwapCheck;

#[async_trait]
impl DoctorCheck for SwapCheck {
    fn name(&self) -> &'static str {
        "Swap Space Availability"
    }

    fn id(&self) -> &'static str {
        "swap"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any
    }

    async fn execute(&self) -> CheckStatus {
        // Read /proc/meminfo to check swap
        match tokio::fs::read_to_string("/proc/meminfo").await {
            Ok(content) => {
                for line in content.lines() {
                    if line.starts_with("SwapTotal:") {
                        // Format: SwapTotal:       16777212 kB
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2
                            && let Ok(swap_kb) = parts[1].parse::<u64>()
                        {
                            if swap_kb == 0 {
                                return CheckStatus::Warning {
                                    message: "No swap space available".to_string(),
                                    fixable: false,
                                };
                            } else {
                                let swap_gb = swap_kb as f64 / (1024.0 * 1024.0);
                                return CheckStatus::Pass(format!(
                                    "Swap space available: {:.2} GB",
                                    swap_gb
                                ));
                            }
                        }
                    }
                }
                CheckStatus::Warning {
                    message: "Could not determine swap status".to_string(),
                    fixable: false,
                }
            }
            Err(e) => CheckStatus::Fail {
                message: format!("Could not read /proc/meminfo: {}", e),
                fixable: false,
            },
        }
    }
}

#[derive(Default)]
pub struct PendingUpdatesCheck;

impl PendingUpdatesCheck {
    const WARN_THRESHOLD: usize = 50;
}

#[async_trait]
impl DoctorCheck for PendingUpdatesCheck {
    fn name(&self) -> &'static str {
        "Pending System Updates"
    }

    fn id(&self) -> &'static str {
        "pending-updates"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Any // checkupdates runs as user
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::Root // pacman -Syu requires root
    }

    async fn execute(&self) -> CheckStatus {
        // Only run on Arch-based systems
        if !crate::common::distro::OperatingSystem::detect().is_arch_based() {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
        }

        // Run checkupdates to get list of pending updates
        let output = TokioCommand::new("checkupdates").output().await;

        match output {
            Ok(output) => {
                // checkupdates exit codes (per man page):
                // 0 = updates available (outputs list)
                // 1 = unknown cause of failure
                // 2 = no updates available
                if output.status.code() == Some(2) {
                    // No updates available
                    return CheckStatus::Pass("System is up to date".to_string());
                }

                if output.status.code() == Some(1) {
                    // Unknown failure - could be temp db issue, network, stale lock, etc.
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let message = if stderr.trim().is_empty() {
                        "checkupdates failed (unknown cause - may be temp db or network issue)"
                            .to_string()
                    } else {
                        format!("checkupdates failed: {}", stderr.trim())
                    };
                    return CheckStatus::Warning {
                        message,
                        fixable: false,
                    };
                }

                if !output.status.success() {
                    return CheckStatus::Fail {
                        message: format!(
                            "checkupdates failed with exit code {:?}",
                            output.status.code()
                        ),
                        fixable: false,
                    };
                }

                // Count the number of pending updates (one per line)
                let update_count = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|line| !line.is_empty())
                    .count();

                if update_count == 0 {
                    CheckStatus::Pass("System is up to date".to_string())
                } else if update_count > Self::WARN_THRESHOLD {
                    CheckStatus::Warning {
                        message: format!(
                            "{} pending updates (exceeds {} threshold)",
                            update_count,
                            Self::WARN_THRESHOLD
                        ),
                        fixable: true,
                    }
                } else {
                    CheckStatus::Pass(format!("{} pending updates", update_count))
                }
            }
            Err(e) => CheckStatus::Fail {
                message: format!("Could not run checkupdates: {}", e),
                fixable: false,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Update system packages with pacman -Syu".to_string())
    }

    async fn fix(&self) -> Result<()> {
        // Run pacman -Syu
        let status = TokioCommand::new("pacman").arg("-Syu").status().await?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("pacman -Syu failed"))
        }
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
        use crate::common::requirements::SMARTMONTOOLS_PACKAGE;
        use crate::common::systemd::{ServiceScope, SystemdManager};

        // First, check if smartctl is available
        if !SMARTMONTOOLS_PACKAGE.is_installed() {
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
        use crate::common::requirements::SMARTMONTOOLS_PACKAGE;
        use crate::common::systemd::{ServiceScope, SystemdManager};

        // Install smartmontools using the standard ensure() flow if not installed
        if !SMARTMONTOOLS_PACKAGE.is_installed() && !SMARTMONTOOLS_PACKAGE.ensure()? {
            return Err(anyhow::anyhow!("smartmontools installation cancelled"));
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
        if !crate::common::distro::OperatingSystem::detect().is_arch_based() {
            return CheckStatus::Skipped("Not an Arch-based system".to_string());
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
