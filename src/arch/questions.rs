use crate::arch::engine::{DataKey, InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::FzfWrapper;
use crate::ui::nerd_font::NerdFont;
use anyhow::{Context, Result};

/// Represents size in megabytes with parsing capabilities
#[derive(Debug, Clone, PartialEq)]
pub struct PartitionSize(u64);

impl PartitionSize {
    /// Parse a size string (e.g., "512M", "1G", "100MB") and return size in MB
    pub fn parse(size_str: &str) -> Option<Self> {
        if size_str.is_empty() {
            return None;
        }

        let size_str = size_str.trim().to_uppercase();

        // Remove any non-alphanumeric characters except digits and common size indicators
        let cleaned: String = size_str.chars()
            .filter(|c| c.is_ascii_digit() || c.is_ascii_alphabetic() || c.is_ascii_whitespace())
            .collect();

        // Try to parse with common suffixes
        if cleaned.ends_with("MB") || cleaned.ends_with("M") {
            if let Ok(size) = cleaned.trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<u64>() {
                return Some(Self(size));
            }
        } else if cleaned.ends_with("GB") || cleaned.ends_with("G") {
            if let Ok(size) = cleaned.trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<u64>() {
                return Some(Self(size * 1024));
            }
        } else if cleaned.ends_with("TB") || cleaned.ends_with("T") {
            if let Ok(size) = cleaned.trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<u64>() {
                return Some(Self(size * 1024 * 1024));
            }
        } else if cleaned.ends_with("KB") || cleaned.ends_with("K") {
            if let Ok(size) = cleaned.trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<u64>() {
                // Convert KB to MB, rounding up
                return Some(Self((size + 1023) / 1024));
            }
        } else {
            // Try to parse as raw number (assume MB)
            if let Ok(size) = size_str.parse::<u64>() {
                return Some(Self(size));
            }
        }

        None
    }

    /// Get the size in megabytes
    pub fn in_mb(&self) -> u64 {
        self.0
    }
}

/// Trait for partition-specific validation
pub trait PartitionValidator {
    /// Validate partition-specific requirements
    fn validate_partition(&self, partition_path: &str, size: Option<PartitionSize>) -> Result<(), String>;
}

/// Default partition validator (no special requirements)
pub struct DefaultPartitionValidator;

impl PartitionValidator for DefaultPartitionValidator {
    fn validate_partition(&self, _partition_path: &str, _size: Option<PartitionSize>) -> Result<(), String> {
        Ok(())
    }
}

/// ESP partition validator with size requirements
pub struct EspPartitionValidator;

impl PartitionValidator for EspPartitionValidator {
    fn validate_partition(&self, _partition_path: &str, size: Option<PartitionSize>) -> Result<(), String> {
        // ESP partition must be at least 100MB for UEFI systems
        if let Some(size) = size {
            if size.in_mb() < 100 {
                return Err(format!(
                    "ESP partition must be at least 100MB. Current size: {}MB",
                    size.in_mb()
                ));
            }
        } else {
            return Err("Could not determine ESP partition size. Please ensure the partition has a valid size.".to_string());
        }
        Ok(())
    }
}

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
        let result = FzfWrapper::builder()
            .prompt(format!(
                "{} Please enter the password for the new user (and root)",
                NerdFont::Lock
            ))
            .password()
            .with_confirmation()
            .password_dialog()?;

        match result {
            crate::menu_utils::FzfResult::Selected(p) => Ok(QuestionResult::Answer(p)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
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

pub struct BooleanQuestion {
    pub id: QuestionId,
    pub prompt: String,
    pub icon: NerdFont,
    pub is_optional: bool,
    pub default_yes: bool,
    pub dynamic_default: Option<Box<dyn Fn(&InstallContext) -> bool + Send + Sync>>,
    pub should_ask_predicate: Option<Box<dyn Fn(&InstallContext) -> bool + Send + Sync>>,
}

impl BooleanQuestion {
    pub fn new(id: QuestionId, prompt: impl Into<String>, icon: NerdFont) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            icon,
            is_optional: false,
            default_yes: false,
            dynamic_default: None,
            should_ask_predicate: None,
        }
    }

    pub fn optional(mut self) -> Self {
        self.is_optional = true;
        self
    }

    pub fn default_yes(mut self) -> Self {
        self.default_yes = true;
        self
    }

    pub fn dynamic_default<F>(mut self, func: F) -> Self
    where
        F: Fn(&InstallContext) -> bool + 'static + Send + Sync,
    {
        self.dynamic_default = Some(Box::new(func));
        self
    }

    pub fn should_ask<F>(mut self, func: F) -> Self
    where
        F: Fn(&InstallContext) -> bool + 'static + Send + Sync,
    {
        self.should_ask_predicate = Some(Box::new(func));
        self
    }
}

#[async_trait::async_trait]
impl Question for BooleanQuestion {
    fn id(&self) -> QuestionId {
        self.id.clone()
    }

    fn is_optional(&self) -> bool {
        self.is_optional
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if let Some(predicate) = &self.should_ask_predicate {
            predicate(context)
        } else {
            true
        }
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        // Determine the effective default based on dynamic function or static setting
        let effective_default = if let Some(dynamic_func) = &self.dynamic_default {
            dynamic_func(context)
        } else {
            self.default_yes
        };

        let options = if effective_default {
            vec!["yes".to_string(), "no".to_string()]
        } else {
            vec!["no".to_string(), "yes".to_string()]
        };

        let result = FzfWrapper::builder()
            .header(format!("{} {}", self.icon, self.prompt))
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
        let result = FzfWrapper::builder()
            .prompt(format!(
                "{} Please enter the encryption password",
                NerdFont::Lock
            ))
            .password()
            .with_confirmation()
            .password_dialog()?;

        match result {
            crate::menu_utils::FzfResult::Selected(p) => Ok(QuestionResult::Answer(p)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

pub struct VirtualBoxWarning;

#[async_trait::async_trait]
impl Question for VirtualBoxWarning {
    fn id(&self) -> QuestionId {
        QuestionId::VirtualBoxWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if let Some(vm_type) = &context.system_info.vm_type {
            let vm = vm_type.to_lowercase();
            vm.contains("oracle") || vm.contains("virtualbox")
        } else {
            false
        }
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        FzfWrapper::message(&format!(
            "{} VirtualBox Detected!\n\n\
             Wayland does not work properly in VirtualBox.\n\
             Please use X11 or another hypervisor for the best experience.",
            NerdFont::Warning
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
    }
}

pub struct WeakPasswordWarning;

#[async_trait::async_trait]
impl Question for WeakPasswordWarning {
    fn id(&self) -> QuestionId {
        QuestionId::WeakPasswordWarning
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        if !context.get_answer_bool(QuestionId::UseEncryption) {
            return false;
        }
        if let Some(pass) = context.get_answer(&QuestionId::EncryptionPassword) {
            pass.len() < 4
        } else {
            false
        }
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        FzfWrapper::message(&format!(
            "{} Weak Password Warning\n\n\
             The encryption password is shorter than 4 characters.\n\
             This is considered insecure.",
            NerdFont::Warning
        ))?;
        Ok(QuestionResult::Answer("acknowledged".to_string()))
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

    #[test]
    fn test_virtualbox_warning_condition() {
        use crate::arch::engine::{InstallContext, Question, QuestionId};
        use crate::arch::questions::VirtualBoxWarning;

        let warning = VirtualBoxWarning;
        let mut context = InstallContext::new();

        // Case 1: No VM
        context.system_info.vm_type = None;
        assert!(!warning.should_ask(&context));

        // Case 2: VirtualBox
        context.system_info.vm_type = Some("Oracle VirtualBox".to_string());
        assert!(warning.should_ask(&context));

        // Case 3: Other VM
        context.system_info.vm_type = Some("KVM".to_string());
        assert!(!warning.should_ask(&context));
    }

    #[test]
    fn test_weak_password_warning_condition() {
        use crate::arch::engine::{InstallContext, Question, QuestionId};
        use crate::arch::questions::WeakPasswordWarning;

        let warning = WeakPasswordWarning;
        let mut context = InstallContext::new();

        // Case 1: Encryption disabled
        context.set_answer(QuestionId::UseEncryption, "false".to_string());
        context.set_answer(QuestionId::EncryptionPassword, "123".to_string());
        assert!(!warning.should_ask(&context));

        // Case 2: Encryption enabled, short password
        context.set_answer(QuestionId::UseEncryption, "true".to_string());
        context.set_answer(QuestionId::EncryptionPassword, "123".to_string());
        assert!(warning.should_ask(&context));

        // Case 3: Encryption enabled, long password
        context.set_answer(QuestionId::EncryptionPassword, "1234".to_string());
        assert!(!warning.should_ask(&context));
    }

    #[test]
    fn test_partition_size_parsing() {
        use super::PartitionSize;

        // Test the PartitionSize parsing functionality
        assert_eq!(PartitionSize::parse("512M"), Some(PartitionSize(512)));
        assert_eq!(PartitionSize::parse("1G"), Some(PartitionSize(1024)));
        assert_eq!(PartitionSize::parse("100MB"), Some(PartitionSize(100)));
        assert_eq!(PartitionSize::parse("1TB"), Some(PartitionSize(1024 * 1024)));
        assert_eq!(PartitionSize::parse("2048KB"), Some(PartitionSize(2))); // 2048KB = 2MB (rounded up)
        assert_eq!(PartitionSize::parse("100"), Some(PartitionSize(100))); // Raw number assumed to be MB
        assert_eq!(PartitionSize::parse(""), None);
        assert_eq!(PartitionSize::parse("invalid"), None);
    }

    #[test]
    fn test_esp_partition_validation() {
        use crate::arch::engine::{InstallContext, Question, QuestionId};
        use crate::arch::questions::PartitionSelectorQuestion;

        let esp_question = PartitionSelectorQuestion::new(
            QuestionId::BootPartition,
            "Select Boot/EFI Partition",
            crate::ui::nerd_font::NerdFont::Folder,
        );

        let mut context = InstallContext::new();

        // Test valid ESP partition (512MB)
        let result = esp_question.validate(&context, "/dev/sda1 (512M)");
        assert!(result.is_ok());

        // Test too small ESP partition (50MB)
        let result = esp_question.validate(&context, "/dev/sda1 (50M)");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ESP partition must be at least 100MB"));

        // Test valid large ESP partition (1G)
        let result = esp_question.validate(&context, "/dev/sda1 (1G)");
        assert!(result.is_ok());

        // Test non-ESP partition (should not trigger ESP validation)
        let root_question = PartitionSelectorQuestion::new(
            QuestionId::RootPartition,
            "Select Root Partition",
            crate::ui::nerd_font::NerdFont::HardDrive,
        );

        let result = root_question.validate(&context, "/dev/sda2 (50M)");
        assert!(result.is_ok()); // Root partition can be any size
    }
}

pub struct PartitioningMethodQuestion;

#[async_trait::async_trait]
impl Question for PartitioningMethodQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::PartitioningMethod
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec![
            "Automatic (Erase Disk)".to_string(),
            "Manual (cfdisk)".to_string(),
        ];

        let result = FzfWrapper::builder()
            .header(format!(
                "{} Select Partitioning Method",
                NerdFont::HardDrive
            ))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(s) => Ok(QuestionResult::Answer(s)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }
}

pub struct RunCfdiskQuestion;

#[async_trait::async_trait]
impl Question for RunCfdiskQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::RunCfdisk
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Manual"))
            .unwrap_or(false)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let disk = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;

        let disk_path = disk.split('(').next().unwrap_or(disk).trim();

        // Check for cfdisk
        if !crate::common::requirements::CFDISK_PACKAGE.is_installed() {
            // Try to install cfdisk if missing
            if let Err(e) = crate::common::requirements::CFDISK_PACKAGE.ensure() {
                return Err(anyhow::anyhow!(
                    "cfdisk is required for manual partitioning but could not be installed: {}",
                    e
                ));
            }
        }

        // Run cfdisk
        // We need to release the terminal for cfdisk
        // But FzfWrapper doesn't hold it.
        // We just run Command with inherit stdio.

        println!("Starting cfdisk on {}...", disk_path);
        println!("Please create your partitions.");
        println!("Press Enter to continue...");
        let _ = std::io::stdin().read_line(&mut String::new());

        let status = std::process::Command::new("cfdisk")
            .arg(disk_path)
            .status()?;

        if !status.success() {
            return Ok(QuestionResult::Cancelled);
        }

        Ok(QuestionResult::Answer("done".to_string()))
    }
}

pub struct PartitionSelectorQuestion {
    pub id: QuestionId,
    pub prompt: String,
    pub icon: NerdFont,
    pub is_optional: bool,
}

impl PartitionSelectorQuestion {
    pub fn new(id: QuestionId, prompt: impl Into<String>, icon: NerdFont) -> Self {
        Self {
            id,
            prompt: prompt.into(),
            icon,
            is_optional: false,
        }
    }

    pub fn optional(mut self) -> Self {
        self.is_optional = true;
        self
    }
}

#[async_trait::async_trait]
impl Question for PartitionSelectorQuestion {
    fn id(&self) -> QuestionId {
        self.id.clone()
    }

    fn is_optional(&self) -> bool {
        self.is_optional
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(|s| s.contains("Manual"))
            .unwrap_or(false)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let disk = context
            .get_answer(&QuestionId::Disk)
            .context("No disk selected")?;
        let disk_path = disk.split('(').next().unwrap_or(disk).trim();

        // Run lsblk to get partitions on this disk
        // We do this here to get fresh data after cfdisk
        let output = std::process::Command::new("lsblk")
            .args(["-n", "-o", "NAME,SIZE,TYPE", "-r", disk_path])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut partitions = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let name = parts[0];
                let size = parts[1];
                let type_ = parts[2];

                if type_ == "part" {
                    // Full path
                    let path = if name.starts_with("/") {
                        name.to_string()
                    } else {
                        format!("/dev/{}", name)
                    };
                    partitions.push(format!("{} ({})", path, size));
                }
            }
        }

        if partitions.is_empty() {
            FzfWrapper::message(&format!(
                "{} No partitions found on {}.\nDid you save your changes in cfdisk?",
                NerdFont::Warning,
                disk_path
            ))?;
            return Ok(QuestionResult::Cancelled);
        }

        let result = FzfWrapper::builder()
            .header(format!("{} {}", self.icon, self.prompt))
            .select(partitions)?;

        match result {
            crate::menu_utils::FzfResult::Selected(s) => Ok(QuestionResult::Answer(s)),
            crate::menu_utils::FzfResult::Cancelled => Ok(QuestionResult::Cancelled),
            _ => Ok(QuestionResult::Cancelled),
        }
    }

    fn validate(&self, context: &InstallContext, answer: &str) -> Result<(), String> {
        // Check if this partition is already used by another answer
        let current_id = self.id();
        let part_path = answer.split('(').next().unwrap_or(answer).trim();

        for (id, val) in &context.answers {
            if id == &current_id {
                continue;
            }

            // Check against other partition questions
            if matches!(
                id,
                QuestionId::RootPartition
                    | QuestionId::BootPartition
                    | QuestionId::HomePartition
                    | QuestionId::SwapPartition
            ) {
                let other_path = val.split('(').next().unwrap_or(val).trim();
                if part_path == other_path {
                    return Err(format!(
                        "Partition {} is already selected for {:?}",
                        part_path, id
                    ));
                }
            }
        }

        // Extract size from the partition description (e.g., "/dev/sda1 (512M)")
        let size_str = answer.split('(').nth(1).and_then(|s| s.split(')').next()).unwrap_or("");
        let size = PartitionSize::parse(size_str.trim());

        // Use appropriate validator based on partition type
        let validator: Box<dyn PartitionValidator> = match current_id {
            QuestionId::BootPartition => Box::new(EspPartitionValidator),
            _ => Box::new(DefaultPartitionValidator),
        };

        validator.validate_partition(part_path, size)?;

        Ok(())
    }
}
