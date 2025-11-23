use crate::arch::engine::{InstallContext, Question, QuestionId};
use crate::menu_utils::FzfWrapper;
use anyhow::Result;

pub struct HostnameQuestion;

#[async_trait::async_trait]
impl Question for HostnameQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Hostname
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        FzfWrapper::input("Please enter the hostname for the new system")
    }

    fn validate(&self, answer: &str) -> Result<(), String> {
        if answer.trim().is_empty() {
            return Err("Hostname cannot be empty.".to_string());
        }
        if answer.contains(' ') {
            return Err("Hostname cannot contain spaces.".to_string());
        }
        Ok(())
    }
}

pub struct UsernameQuestion;

#[async_trait::async_trait]
impl Question for UsernameQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Username
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        FzfWrapper::input("Please enter the username for the new user")
    }

    fn validate(&self, answer: &str) -> Result<(), String> {
        if answer.trim().is_empty() {
            return Err("Username cannot be empty.".to_string());
        }
        if answer.contains(' ') {
            return Err("Username cannot contain spaces.".to_string());
        }
        if answer == "root" {
            return Err("Username cannot be 'root'.".to_string());
        }
        Ok(())
    }
}

pub struct MirrorRegionQuestion;

#[async_trait::async_trait]
impl Question for MirrorRegionQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::MirrorRegion
    }

    fn is_ready(&self, context: &InstallContext) -> bool {
        let data = context.data.lock().unwrap();
        data.contains_key("mirror_regions")
    }

    async fn ask(&self, context: &InstallContext) -> Result<String> {
        let data = context.data.lock().unwrap();
        let regions_str = data.get("mirror_regions").unwrap();
        let regions: Vec<String> = regions_str.split(',').map(|s| s.to_string()).collect();

        let result = FzfWrapper::builder()
            .header("Select Mirror Region")
            .select(regions)?;

        match result {
            crate::menu_utils::FzfResult::Selected(region) => Ok(region),
            _ => Ok("".to_string()), // Handle cancellation better?
        }
    }

    fn validate(&self, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a mirror region.".to_string());
        }
        Ok(())
    }
}

pub struct TimezoneQuestion;

#[async_trait::async_trait]
impl Question for TimezoneQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Timezone
    }

    fn is_ready(&self, context: &InstallContext) -> bool {
        let data = context.data.lock().unwrap();
        data.contains_key("timezones")
    }

    async fn ask(&self, context: &InstallContext) -> Result<String> {
        let data = context.data.lock().unwrap();
        let timezones_str = data.get("timezones").unwrap();
        let timezones: Vec<String> = timezones_str.lines().map(|s| s.to_string()).collect();

        let result = FzfWrapper::builder()
            .header("Select Timezone")
            .select(timezones)?;

        match result {
            crate::menu_utils::FzfResult::Selected(tz) => Ok(tz),
            _ => Ok("".to_string()),
        }
    }

    fn validate(&self, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a timezone.".to_string());
        }
        Ok(())
    }
}

pub struct DiskQuestion;

#[async_trait::async_trait]
impl Question for DiskQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Disk
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        // Mock disks for now
        let disks = vec![
            "/dev/sda (500GB)".to_string(),
            "/dev/nvme0n1 (1TB)".to_string(),
        ];

        let result = FzfWrapper::builder()
            .header("Select Disk to Install To")
            .select(disks)?;

        match result {
            crate::menu_utils::FzfResult::Selected(disk) => Ok(disk),
            _ => Ok("".to_string()),
        }
    }

    fn validate(&self, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a disk.".to_string());
        }
        // Example: Check if disk actually exists (mocked here)
        if !answer.starts_with("/dev/") {
            return Err("Invalid disk path.".to_string());
        }
        Ok(())
    }
}

pub struct KeymapQuestion;

#[async_trait::async_trait]
impl Question for KeymapQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Keymap
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        // Mock keymaps
        let keymaps = vec!["us".to_string(), "de-latin1".to_string(), "uk".to_string()];

        let result = FzfWrapper::builder()
            .header("Select Keymap")
            .select(keymaps)?;

        match result {
            crate::menu_utils::FzfResult::Selected(km) => Ok(km),
            _ => Ok("".to_string()),
        }
    }
}

pub struct LocaleQuestion;

#[async_trait::async_trait]
impl Question for LocaleQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Locale
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        // Mock common locales
        let common_locales = vec![
            "en_US.UTF-8".to_string(),
            "de_DE.UTF-8".to_string(),
            "fr_FR.UTF-8".to_string(),
            "es_ES.UTF-8".to_string(),
            "ja_JP.UTF-8".to_string(),
        ];

        let result = FzfWrapper::builder()
            .header("Select System Locale")
            .select(common_locales)?;

        match result {
            crate::menu_utils::FzfResult::Selected(locale) => Ok(locale),
            _ => Ok("en_US.UTF-8".to_string()), // Default fallback
        }
    }
}

pub struct PasswordQuestion;

#[async_trait::async_trait]
impl Question for PasswordQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Password
    }

    fn is_ready(&self, _context: &InstallContext) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<String> {
        loop {
            let pass1 =
                FzfWrapper::password("Please enter the password for the new user (and root)")?;
            if pass1.is_empty() {
                FzfWrapper::message("Password cannot be empty.")?;
                continue;
            }

            let pass2 = FzfWrapper::password("Please confirm the password")?;

            if pass1 == pass2 {
                return Ok(pass1);
            } else {
                FzfWrapper::message("Passwords do not match. Please try again.")?;
            }
        }
    }

    // No extra validate() needed as ask() handles the confirmation loop,
    // but we could add complexity checks here if desired.
}
