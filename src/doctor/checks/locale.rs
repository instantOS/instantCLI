use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

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
