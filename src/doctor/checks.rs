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
        if !crate::common::distro::is_instantos() {
            return CheckStatus::Pass("Not running on instantOS (check skipped)".to_string());
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
                    CheckStatus::Fail {
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
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            if metadata.is_file() {
                total_size += metadata.len();
            } else if metadata.is_dir() {
                dirs_to_visit.push(entry.path());
            }
        }
    }

    Ok(total_size)
}
