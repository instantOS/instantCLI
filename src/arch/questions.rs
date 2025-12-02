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
}

#[async_trait::async_trait]
impl Question for BooleanQuestion {
    fn id(&self) -> QuestionId {
        self.id.clone()
    }

    fn is_optional(&self) -> bool {
        self.is_optional
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
}
