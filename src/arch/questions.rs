use crate::arch::engine::{InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;

pub struct HostnameQuestion;

#[async_trait::async_trait]
impl Question for HostnameQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Hostname
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let result = FzfWrapper::builder()
            .prompt(format!(
                "{} Please enter the hostname for the new system",
                NerdFont::Desktop
            ))
            .input()
            .input_result()?;

        match result {
            crate::menu_utils::FzfResult::Selected(s) => Ok(QuestionResult::Answer(s)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
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

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let result = FzfWrapper::builder()
            .prompt(format!(
                "{} Please enter the username for the new user",
                NerdFont::User
            ))
            .input()
            .input_result()?;

        match result {
            crate::menu_utils::FzfResult::Selected(s) => Ok(QuestionResult::Answer(s)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
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

    fn required_data_keys(&self) -> Vec<String> {
        vec!["mirror_regions".to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let data = context.data.lock().unwrap();
        let regions_str = data.get("mirror_regions").unwrap();
        let regions: Vec<String> = regions_str.split(',').map(|s| s.to_string()).collect();

        let result = FzfWrapper::builder()
            .header(format!("{} Select Mirror Region", NerdFont::Globe))
            .select(regions)?;

        match result {
            crate::menu_utils::FzfResult::Selected(region) => Ok(QuestionResult::Answer(region)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a mirror region.".to_string());
        }
        Ok(())
    }

    fn data_providers(&self) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
        vec![Box::new(crate::arch::mirrors::MirrorlistProvider)]
    }
}

pub struct TimezoneQuestion;

#[async_trait::async_trait]
impl Question for TimezoneQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Timezone
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec!["timezones".to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let data = context.data.lock().unwrap();
        let timezones_str = data.get("timezones").unwrap();
        let timezones: Vec<String> = timezones_str.lines().map(|s| s.to_string()).collect();

        let result = FzfWrapper::builder()
            .header(format!("{} Select Timezone", NerdFont::Clock))
            .select(timezones)?;

        match result {
            crate::menu_utils::FzfResult::Selected(tz) => Ok(QuestionResult::Answer(tz)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a timezone.".to_string());
        }
        Ok(())
    }

    fn data_providers(&self) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
        vec![Box::new(crate::arch::timezones::TimezoneProvider)]
    }
}

pub struct DiskQuestion;

#[async_trait::async_trait]
impl Question for DiskQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Disk
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec!["disks".to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let data = context.data.lock().unwrap();
        let disks_str = data.get("disks").unwrap();
        let disks: Vec<String> = disks_str.lines().map(|s| s.to_string()).collect();

        if disks.is_empty() {
            return Ok(QuestionResult::Cancelled); // Or show error
        }

        let result = FzfWrapper::builder()
            .header(format!("{} Select Disk to Install To", NerdFont::HardDrive))
            .select(disks)?;

        match result {
            crate::menu_utils::FzfResult::Selected(disk) => Ok(QuestionResult::Answer(disk)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a disk.".to_string());
        }
        Ok(())
    }

    fn data_providers(&self) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
        vec![Box::new(crate::arch::disks::DiskProvider)]
    }
}

pub struct KeymapQuestion;

#[async_trait::async_trait]
impl Question for KeymapQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Keymap
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        // Mock keymaps
        let keymaps = vec!["us".to_string(), "de-latin1".to_string(), "uk".to_string()];

        let result = FzfWrapper::builder()
            .header(format!("{} Select Keymap", NerdFont::Key))
            .select(keymaps)?;

        match result {
            crate::menu_utils::FzfResult::Selected(km) => Ok(QuestionResult::Answer(km)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

pub struct LocaleQuestion;

#[async_trait::async_trait]
impl Question for LocaleQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Locale
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec!["locales".to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let data = context.data.lock().unwrap();
        let locales_str = data.get("locales").unwrap();
        let locales: Vec<String> = locales_str.lines().map(|s| s.to_string()).collect();

        if locales.is_empty() {
            return Ok(QuestionResult::Cancelled);
        }

        let result = FzfWrapper::builder()
            .header(format!("{} Select System Locale", NerdFont::Flag))
            .select(locales)?;

        match result {
            crate::menu_utils::FzfResult::Selected(locale) => Ok(QuestionResult::Answer(locale)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn data_providers(&self) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
        vec![Box::new(crate::arch::locales::LocaleProvider)]
    }
}

pub struct PasswordQuestion;

#[async_trait::async_trait]
impl Question for PasswordQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Password
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        loop {
            // NOTE: FzfWrapper::password currently returns Result<String>, not FzfResult.
            // It uses `gum` or fallback. `gum` cancellation returns error or empty string?
            // FzfWrapper::password implementation calls `execute_password`.
            // `execute_password` returns `Ok(stdout.trim())` or fallback.
            // If gum is cancelled (Ctrl+C), it might return error or empty.
            // For now, let's assume if we get empty string (and we require password), it's a cancel?
            // But password CAN be empty? No, we validate against it.
            // So if `password()` returns empty, we can treat as cancel?
            // But `PasswordQuestion` has its own loop for confirmation.

            // Let's try to use `password()` and if it returns empty, treat as cancel.

            let pass1 = match FzfWrapper::password(&format!(
                "{} Please enter the password for the new user (and root)",
                NerdFont::Lock
            )) {
                Ok(p) if p.is_empty() => return Ok(QuestionResult::Cancelled),
                Ok(p) => p,
                Err(_) => return Ok(QuestionResult::Cancelled),
            };

            let pass2 = match FzfWrapper::password(&format!(
                "{} Please confirm the password",
                NerdFont::Check
            )) {
                Ok(p) if p.is_empty() => return Ok(QuestionResult::Cancelled),
                Ok(p) => p,
                Err(_) => return Ok(QuestionResult::Cancelled),
            };

            if pass1 == pass2 {
                return Ok(QuestionResult::Answer(pass1));
            } else {
                FzfWrapper::message(&format!(
                    "{} Passwords do not match. Please try again.",
                    NerdFont::Warning
                ))?;
            }
        }
    }

    // No extra validate() needed as ask() handles the confirmation loop,
    // but we could add complexity checks here if desired.
}
