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
