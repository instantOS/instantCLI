use crate::arch::engine::{DataKey, InstallContext, Question, QuestionId, QuestionResult};
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
        vec![crate::arch::mirrors::MirrorRegionsKey::KEY.to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let regions = context
            .get::<crate::arch::mirrors::MirrorRegionsKey>()
            .unwrap_or_default();

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
        vec![crate::arch::timezones::TimezonesKey::KEY.to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let timezones = context
            .get::<crate::arch::timezones::TimezonesKey>()
            .unwrap_or_default();

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
        vec![crate::arch::disks::DisksKey::KEY.to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let disks = context
            .get::<crate::arch::disks::DisksKey>()
            .unwrap_or_default();

        if disks.is_empty() {
            return Ok(QuestionResult::Cancelled);
        }

        let result = FzfWrapper::builder()
            .header(format!("{} Select Installation Disk", NerdFont::HardDrive))
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
        if !answer.starts_with("/dev/") {
            return Err("Invalid disk selection: must start with /dev/".to_string());
        }

        // Extract device name from the selection (e.g., "/dev/sda (500 GiB)" -> "/dev/sda")
        let device_name = answer.split('(').next().unwrap_or(answer).trim();

        // Get the root filesystem device to check against
        if let Ok(Some(root_device)) = crate::arch::disks::get_root_device() {
            // Check if the selected device is exactly the root filesystem device
            if device_name == root_device {
                return Err(format!(
                    "Cannot select the current root filesystem device ({}) for installation.\n\
                    This device contains the currently running system and would cause data loss.\n\
                    Please select a different disk.",
                    root_device
                ));
            }
        }

        // Check if this disk is the current boot disk (physical disk containing root)
        if let Ok(Some(boot_disk)) = crate::arch::disks::get_boot_disk()
            && device_name == boot_disk
        {
            return Err(format!(
                "Cannot select the current boot disk ({}) for installation.\n\
                    This disk contains the currently running system and would cause data loss.\n\
                    Please select a different disk.",
                boot_disk
            ));
        }

        // Check if disk is mounted
        if let Ok(true) = crate::arch::disks::is_disk_mounted(device_name) {
            return Err(format!(
                "The selected disk ({}) contains mounted partitions.\n\
                Please unmount all partitions on this disk before proceeding.",
                device_name
            ));
        }

        // Check if disk is used as swap
        if let Ok(true) = crate::arch::disks::is_disk_swap(device_name) {
            return Err(format!(
                "The selected disk ({}) is currently being used as swap.\n\
                Please swapoff this disk before proceeding.",
                device_name
            ));
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

    fn required_data_keys(&self) -> Vec<String> {
        vec![crate::arch::keymaps::KeymapsKey::KEY.to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let keymaps = context
            .get::<crate::arch::keymaps::KeymapsKey>()
            .unwrap_or_default();

        if keymaps.is_empty() {
            return Ok(QuestionResult::Cancelled);
        }

        let result = FzfWrapper::builder()
            .header(format!("{} Select Keymap", NerdFont::Keyboard))
            .select(keymaps)?;

        match result {
            crate::menu_utils::FzfResult::Selected(val) => Ok(QuestionResult::Answer(val.value)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn data_providers(&self) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
        vec![Box::new(crate::arch::keymaps::KeymapProvider)]
    }
}

pub struct LocaleQuestion;

#[async_trait::async_trait]
impl Question for LocaleQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Locale
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec![crate::arch::locales::LocalesKey::KEY.to_string()]
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let locales = context
            .get::<crate::arch::locales::LocalesKey>()
            .unwrap_or_default();

        if locales.is_empty() {
            return Ok(QuestionResult::Cancelled);
        }

        let result = FzfWrapper::builder()
            .header(format!("{} Select System Locale", NerdFont::Language))
            .select(locales)?;

        match result {
            crate::menu_utils::FzfResult::Selected(val) => Ok(QuestionResult::Answer(val.value)),
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

    fn is_sensitive(&self) -> bool {
        true
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

pub struct KernelQuestion;

#[async_trait::async_trait]
impl Question for KernelQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Kernel
    }

    fn is_optional(&self) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let kernels = vec![
            "linux".to_string(),
            "linux-lts".to_string(),
            "linux-zen".to_string(),
        ];

        let result = FzfWrapper::builder()
            .header(format!("{} Select Kernel", NerdFont::Gear))
            .select(kernels)?;

        match result {
            crate::menu_utils::FzfResult::Selected(k) => Ok(QuestionResult::Answer(k)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        if answer.is_empty() {
            return Err("You must select a kernel.".to_string());
        }
        Ok(())
    }
}

pub struct UseEncryptionQuestion;

#[async_trait::async_trait]
impl Question for UseEncryptionQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::UseEncryption
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec!["no".to_string(), "yes".to_string()];

        let result = FzfWrapper::builder()
            .header(format!("{} Encrypt the installation disk?", NerdFont::Lock))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(ans) => Ok(QuestionResult::Answer(ans)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

pub struct UsePlymouthQuestion;

#[async_trait::async_trait]
impl Question for UsePlymouthQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::UsePlymouth
    }

    fn is_optional(&self) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec!["no".to_string(), "yes".to_string()];

        let result = FzfWrapper::builder()
            .header(format!("{} Enable Plymouth boot splash screen?", NerdFont::Monitor))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(ans) => Ok(QuestionResult::Answer(ans)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

pub struct EncryptionPasswordQuestion;

#[async_trait::async_trait]
impl Question for EncryptionPasswordQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::EncryptionPassword
    }

    fn is_sensitive(&self) -> bool {
        true
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context.get_answer_bool(QuestionId::UseEncryption)
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        loop {
            let pass1 = match FzfWrapper::password(&format!(
                "{} Please enter the encryption password",
                NerdFont::Lock
            )) {
                Ok(p) if p.is_empty() => return Ok(QuestionResult::Cancelled),
                Ok(p) => p,
                Err(_) => return Ok(QuestionResult::Cancelled),
            };

            let pass2 = match FzfWrapper::password(&format!(
                "{} Please confirm the encryption password",
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
}

pub struct LogUploadQuestion;

#[async_trait::async_trait]
impl Question for LogUploadQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::LogUpload
    }

    fn is_optional(&self) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec!["yes".to_string(), "no".to_string()];

        let result = FzfWrapper::builder()
            .header(format!(
                "{} Upload installation logs to snips.sh?",
                NerdFont::Debug
            ))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(ans) => Ok(QuestionResult::Answer(ans)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_device_name_extraction() {
        // Test that device name extraction works correctly
        let test_cases = vec![
            ("/dev/sda (500 GiB)", "/dev/sda"),
            ("/dev/nvme0n1 (1 TiB)", "/dev/nvme0n1"),
            ("/dev/sdb", "/dev/sdb"),
            ("/dev/sdc   ", "/dev/sdc"),
        ];

        for (input, expected) in test_cases {
            let device_name = input.split('(').next().unwrap_or(input).trim();
            assert_eq!(device_name, expected, "Failed for input: {}", input);
        }
    }
}
