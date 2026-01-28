use crate::arch::annotations::AnnotatedValue;
use crate::arch::engine::{DataKey, InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::preview::{PreviewId, preview_command};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::Result;

#[derive(Clone)]
struct MirrorRegionOption {
    name: String,
}

impl MirrorRegionOption {
    fn new(name: String) -> Self {
        Self { name }
    }
}

impl FzfSelectable for MirrorRegionOption {
    fn fzf_display_text(&self) -> String {
        self.name.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Globe, "Mirror Region")
            .subtext("Select the closest region for faster downloads.")
            .blank()
            .field("Region", &self.name)
            .blank()
            .line(colors::TEAL, None, "Notes")
            .bullets([
                "Used to generate the pacman mirrorlist",
                "You can change mirrors later",
            ])
            .build()
    }

    fn fzf_key(&self) -> String {
        self.name.clone()
    }
}

#[derive(Clone)]
struct TimezoneOption {
    value: String,
}

impl FzfSelectable for TimezoneOption {
    fn fzf_display_text(&self) -> String {
        self.value.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::Command(preview_command(PreviewId::Timezone))
    }

    fn fzf_key(&self) -> String {
        self.value.clone()
    }
}

#[derive(Clone)]
struct LocaleOption {
    value: String,
    annotation: Option<String>,
}

impl From<AnnotatedValue<String>> for LocaleOption {
    fn from(value: AnnotatedValue<String>) -> Self {
        Self {
            value: value.value,
            annotation: value.annotation,
        }
    }
}

impl FzfSelectable for LocaleOption {
    fn fzf_display_text(&self) -> String {
        match &self.annotation {
            Some(label) => format!("{} - {}", label, self.value),
            None => self.value.clone(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Language, "Locale")
            .subtext("Sets system language and formatting.")
            .blank()
            .field("Locale", &self.value);

        if let Some(label) = &self.annotation {
            builder = builder.field("Language", label);
        }

        builder
            .blank()
            .line(colors::TEAL, None, "Used for")
            .bullets(["System messages", "Date and number formatting"])
            .build()
    }

    fn fzf_key(&self) -> String {
        self.value.clone()
    }
}

#[derive(Clone)]
struct KeymapOption {
    value: String,
    annotation: Option<String>,
}

impl From<AnnotatedValue<String>> for KeymapOption {
    fn from(value: AnnotatedValue<String>) -> Self {
        Self {
            value: value.value,
            annotation: value.annotation,
        }
    }
}

impl FzfSelectable for KeymapOption {
    fn fzf_display_text(&self) -> String {
        match &self.annotation {
            Some(label) => format!("{} - {}", label, self.value),
            None => self.value.clone(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Keyboard, "Keymap")
            .subtext("Sets the console keyboard layout for the system.")
            .blank()
            .field("Keymap", &self.value);

        if let Some(label) = &self.annotation {
            builder = builder.field("Layout", label);
        }

        builder
            .blank()
            .line(colors::TEAL, None, "Notes")
            .bullets([
                "Affects the installer and TTYs",
                "Desktop layout can be changed later",
            ])
            .build()
    }

    fn fzf_key(&self) -> String {
        self.value.clone()
    }
}

#[derive(Clone)]
enum KernelOption {
    Linux,
    Lts,
    Zen,
}

impl KernelOption {
    fn label(&self) -> &'static str {
        match self {
            KernelOption::Linux => "linux",
            KernelOption::Lts => "linux-lts",
            KernelOption::Zen => "linux-zen",
        }
    }

    fn preview(&self) -> FzfPreview {
        match self {
            KernelOption::Linux => PreviewBuilder::new()
                .header(NerdFont::Gear, "linux")
                .subtext("The standard Arch kernel with the latest updates.")
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets(["Most systems", "Up-to-date hardware support"])
                .build(),
            KernelOption::Lts => PreviewBuilder::new()
                .header(NerdFont::Gear, "linux-lts")
                .subtext("Long-term support kernel with fewer breaking changes.")
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets(["Stability", "Older hardware"])
                .build(),
            KernelOption::Zen => PreviewBuilder::new()
                .header(NerdFont::Gear, "linux-zen")
                .subtext("Performance-tuned kernel with extra desktop patches.")
                .blank()
                .line(colors::TEAL, None, "Best for")
                .bullets(["Responsive desktop feel", "Gaming"])
                .build(),
        }
    }
}

impl FzfSelectable for KernelOption {
    fn fzf_display_text(&self) -> String {
        self.label().to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }

    fn fzf_key(&self) -> String {
        self.label().to_string()
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

    /// Skip this question if mirror regions fetch failed.
    /// Installation will proceed with fallback mirrorlist.
    fn should_ask(&self, context: &InstallContext) -> bool {
        // If the fetch failed, skip this question
        !context
            .get::<crate::arch::mirrors::MirrorRegionsFetchFailed>()
            .unwrap_or(false)
    }

    async fn ask(&self, context: &InstallContext) -> Result<QuestionResult> {
        let regions = context
            .get::<crate::arch::mirrors::MirrorRegionsKey>()
            .unwrap_or_default();

        // Defensive: if somehow we got here with no regions, cancel
        if regions.is_empty() {
            return Ok(QuestionResult::Cancelled);
        }

        let options: Vec<MirrorRegionOption> =
            regions.into_iter().map(MirrorRegionOption::new).collect();

        let result = FzfWrapper::builder()
            .header(format!("{} Select Mirror Region", NerdFont::Globe))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(region) => {
                Ok(QuestionResult::Answer(region.name))
            }
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

        let options: Vec<TimezoneOption> = timezones
            .into_iter()
            .map(|value| TimezoneOption { value })
            .collect();

        let result = FzfWrapper::builder()
            .header(format!("{} Select Timezone", NerdFont::Clock))
            .select(options)?;

        match result {
            crate::menu_utils::FzfResult::Selected(tz) => Ok(QuestionResult::Answer(tz.value)),
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

        let options: Vec<KeymapOption> = keymaps.into_iter().map(KeymapOption::from).collect();

        let result = FzfWrapper::builder()
            .header(format!("{} Select Keymap", NerdFont::Keyboard))
            .select(options)?;

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

        let options: Vec<LocaleOption> = locales.into_iter().map(LocaleOption::from).collect();

        let result = FzfWrapper::builder()
            .header(format!("{} Select System Locale", NerdFont::Language))
            .select(options)?;

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
        let kernels = vec![KernelOption::Linux, KernelOption::Lts, KernelOption::Zen];

        let result = FzfWrapper::builder()
            .header(format!("{} Select Kernel", NerdFont::Gear))
            .select(kernels)?;

        match result {
            crate::menu_utils::FzfResult::Selected(k) => {
                Ok(QuestionResult::Answer(k.label().to_string()))
            }
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
