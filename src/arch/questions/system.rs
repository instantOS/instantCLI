use super::text_input::{TextInputQuestion, validators};
use crate::arch::annotations::AnnotatedValue;
use crate::arch::config::DesktopEnvironment;
use crate::arch::engine::{DataKey, InstallContext, Question, QuestionId, QuestionResult};
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, HeaderBuilder, MenuPresentation};
use crate::preview::{PreviewId, preview_command};
use crate::settings::definitions::system::validate_hostname;
use crate::settings::users::validate_username;
use crate::ui::catppuccin::{colors, format_icon_colored};
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
}

/// Presentation metadata for a list of [`AnnotatedOption`]s. The values and
/// annotations come from the context; this describes only the preview prose.
struct AnnotatedOptionStyle {
    icon: NerdFont,
    title: &'static str,
    subtext: &'static str,
    field: &'static str,
    annotation_field: &'static str,
    notes_label: &'static str,
    notes: &'static [&'static str],
}

const LOCALE_OPTION_STYLE: AnnotatedOptionStyle = AnnotatedOptionStyle {
    icon: NerdFont::Language,
    title: "Locale",
    subtext: "Sets system language and formatting.",
    field: "Locale",
    annotation_field: "Language",
    notes_label: "Used for",
    notes: &["System messages", "Date and number formatting"],
};

const KEYMAP_OPTION_STYLE: AnnotatedOptionStyle = AnnotatedOptionStyle {
    icon: NerdFont::Keyboard,
    title: "Keymap",
    subtext: "Sets the console keyboard layout for the system.",
    field: "Keymap",
    annotation_field: "Layout",
    notes_label: "Notes",
    notes: &[
        "Affects the installer and TTYs",
        "Desktop layout can be changed later",
    ],
};

/// A selectable entry backed by an annotated value (e.g. locales, keymaps).
#[derive(Clone)]
struct AnnotatedOption {
    value: String,
    annotation: Option<String>,
    style: &'static AnnotatedOptionStyle,
}

impl AnnotatedOption {
    fn new(value: AnnotatedValue<String>, style: &'static AnnotatedOptionStyle) -> Self {
        Self {
            value: value.value,
            annotation: value.annotation,
            style,
        }
    }
}

impl FzfSelectable for AnnotatedOption {
    fn fzf_display_text(&self) -> String {
        match &self.annotation {
            Some(label) => format!("{} - {}", label, self.value),
            None => self.value.clone(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let style = self.style;
        let mut builder = PreviewBuilder::new()
            .header(style.icon, style.title)
            .subtext(style.subtext)
            .blank()
            .field(style.field, &self.value);

        if let Some(label) = &self.annotation {
            builder = builder.field(style.annotation_field, label);
        }

        builder
            .blank()
            .line(colors::TEAL, None, style.notes_label)
            .bullets(style.notes.iter().copied())
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
        let icon = match self {
            Self::Linux => format_icon_colored(NerdFont::LinuxTux, colors::TEXT),
            Self::Lts => format_icon_colored(NerdFont::Shield, colors::TEAL),
            Self::Zen => format_icon_colored(NerdFont::Performance, colors::MAUVE),
        };
        format!("{icon} {}", self.label())
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview()
    }
}

fn add_desktop_environment_disclaimer(builder: PreviewBuilder) -> PreviewBuilder {
    builder
        .blank()
        .separator()
        .blank()
        .subtext(
            "These desktop environments are not mutually exclusive and can be installed next to each other at any time.",
        )
}

fn desktop_environment_preview(environment: DesktopEnvironment) -> FzfPreview {
    let builder = match environment {
        DesktopEnvironment::Sway => PreviewBuilder::new()
            .header(NerdFont::Desktop, "Sway")
            .subtext("Keyboard-driven wlroots compositor with an i3-like workflow.")
            .blank()
            .line(colors::TEAL, None, "Good fit for")
            .bullets([
                "Stable tiling Wayland setups",
                "Users who want predictable, scriptable behavior",
            ]),
        DesktopEnvironment::Niri => PreviewBuilder::new()
            .header(NerdFont::Desktop, "niri")
            .subtext("Scrollable-tiling Wayland compositor with a column-based workflow.")
            .blank()
            .line(colors::TEAL, None, "Good fit for")
            .bullets([
                "Large or multiple monitors",
                "Users who want dynamic workspaces without manual layout juggling",
            ]),
        DesktopEnvironment::InstantWM => PreviewBuilder::new()
            .header(NerdFont::Desktop, "instantWM")
            .subtext("The instantOS compositor and the default instantOS desktop.")
            .blank()
            .line(colors::TEAL, None, "Good fit for")
            .bullets([
                "The classic instantOS tiling workflow",
                "An integrated, ready-to-use instantOS experience",
                "Both X11 and Wayland sessions",
            ]),
        DesktopEnvironment::Hyprland => PreviewBuilder::new()
            .header(NerdFont::Desktop, "Hyprland")
            .subtext("Animated dynamic-tiling Wayland compositor with a more visual style.")
            .blank()
            .line(colors::TEAL, None, "Good fit for")
            .bullets([
                "Users who want polished motion and eye candy",
                "Flexible tiling with a more opinionated feel",
            ]),
        DesktopEnvironment::Tty => PreviewBuilder::new()
            .header(NerdFont::Terminal, "None / TTY")
            .subtext("Install without a graphical desktop session as the default.")
            .blank()
            .line(colors::TEAL, None, "What happens")
            .bullets([
                "No default LightDM desktop session is configured",
                "The system boots to a text login and shell-first workflow",
            ]),
    };

    add_desktop_environment_disclaimer(builder).build()
}

impl FzfSelectable for DesktopEnvironment {
    fn fzf_display_text(&self) -> String {
        let icon = match self {
            Self::InstantWM => format_icon_colored(NerdFont::ViewQuilt, colors::PEACH),
            Self::Sway => format_icon_colored(NerdFont::Waves, colors::BLUE),
            Self::Niri => format_icon_colored(NerdFont::Columns, colors::TEAL),
            Self::Hyprland => format_icon_colored(NerdFont::Desktop, colors::MAUVE),
            Self::Tty => format_icon_colored(NerdFont::Terminal, colors::OVERLAY0),
        };
        format!("{icon} {}", self.label())
    }

    fn fzf_preview(&self) -> FzfPreview {
        desktop_environment_preview(*self)
    }

    fn fzf_key(&self) -> String {
        self.answer_value().to_string()
    }
}

pub struct DesktopEnvironmentQuestion;

#[async_trait::async_trait]
impl Question for DesktopEnvironmentQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::DesktopEnvironment
    }

    fn is_optional(&self) -> bool {
        true
    }

    fn description(&self) -> Option<&str> {
        Some("Choose your desktop environment")
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let options = vec![
            DesktopEnvironment::InstantWM,
            DesktopEnvironment::Sway,
            DesktopEnvironment::Niri,
            DesktopEnvironment::Hyprland,
            DesktopEnvironment::Tty,
        ];

        let result = FzfWrapper::builder()
            .header(HeaderBuilder::new(NerdFont::Desktop, "Select Desktop Environment").build())
            .presentation(MenuPresentation::Padded)
            .select(options)?;

        Ok(QuestionResult::from_selection(result, |environment| {
            environment.answer_value().to_string()
        }))
    }

    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        Some(DesktopEnvironment::DEFAULT.answer_value().to_string())
    }

    fn validate(&self, _context: &InstallContext, answer: &str) -> Result<(), String> {
        match answer {
            "sway" | "niri" | "instantwm" | "hyprland" | "none/tty" => Ok(()),
            _ => Err("You must select a desktop environment.".to_string()),
        }
    }
}

/// Hostname rules live in `settings::definitions::system::validate_hostname`
/// (same rules as `ins settings` hostname editing).
pub fn hostname_question() -> TextInputQuestion {
    TextInputQuestion::new(
        QuestionId::Hostname,
        "Please enter the hostname for the new system",
        NerdFont::Desktop,
    )
    .description("Set the system's network hostname")
    .validator(|answer| validate_hostname(answer).map_err(|error| error.to_string()))
}

/// Username rules live in `settings::users::validate_username` (same rules as
/// user management in `ins settings`).
pub fn username_question() -> TextInputQuestion {
    TextInputQuestion::new(
        QuestionId::Username,
        "Please enter the username for the new user",
        NerdFont::User,
    )
    .description("Create the main user account")
    .validator(|answer| validate_username(answer).map_err(|error| error.to_string()))
    .validator(validators::forbidden_value("Username", "root"))
}

pub struct MirrorRegionQuestion;

#[async_trait::async_trait]
impl Question for MirrorRegionQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::MirrorRegion
    }

    fn description(&self) -> Option<&str> {
        Some("Select the closest mirror region for faster downloads")
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
            .header(HeaderBuilder::new(NerdFont::Globe, "Select Mirror Region").build())
            .select(options)?;

        Ok(QuestionResult::from_selection(result, |region| region.name))
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

    fn description(&self) -> Option<&str> {
        Some("Set the system timezone")
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
            .header(HeaderBuilder::new(NerdFont::Clock, "Select Timezone").build())
            .select(options)?;

        Ok(QuestionResult::from_selection(result, |tz| tz.value))
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

    fn description(&self) -> Option<&str> {
        Some("Set the console keyboard layout")
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

        let options: Vec<AnnotatedOption> = keymaps
            .into_iter()
            .map(|value| AnnotatedOption::new(value, &KEYMAP_OPTION_STYLE))
            .collect();

        let result = FzfWrapper::builder()
            .header(HeaderBuilder::new(NerdFont::Keyboard, "Select Keymap").build())
            .select(options)?;

        Ok(QuestionResult::from_selection(result, |val| val.value))
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

    fn description(&self) -> Option<&str> {
        Some("Set the system language and formatting")
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

        let options: Vec<AnnotatedOption> = locales
            .into_iter()
            .map(|value| AnnotatedOption::new(value, &LOCALE_OPTION_STYLE))
            .collect();

        let result = FzfWrapper::builder()
            .header(HeaderBuilder::new(NerdFont::Language, "Select System Locale").build())
            .select(options)?;

        Ok(QuestionResult::from_selection(result, |val| val.value))
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

    fn description(&self) -> Option<&str> {
        Some("Set the password for the new user and root account")
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

        Ok(QuestionResult::from_selection(result, |p| p))
    }
}

pub struct KernelQuestion;

#[async_trait::async_trait]
impl Question for KernelQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Kernel
    }

    fn description(&self) -> Option<&str> {
        Some("Select the Linux kernel variant")
    }

    fn is_optional(&self) -> bool {
        true
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        let kernels = vec![KernelOption::Linux, KernelOption::Lts, KernelOption::Zen];

        let result = FzfWrapper::builder()
            .header(HeaderBuilder::new(NerdFont::Gear, "Select Kernel").build())
            .presentation(MenuPresentation::Padded)
            .select(kernels)?;

        Ok(QuestionResult::from_selection(result, |k| {
            k.label().to_string()
        }))
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

    fn description(&self) -> Option<&str> {
        Some("Set the disk encryption password")
    }

    fn is_sensitive(&self) -> bool {
        true
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        context.get_answer_bool(QuestionId::UseEncryption)
    }

    fn depends_on(&self) -> &[QuestionId] {
        &[QuestionId::UseEncryption]
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

        Ok(QuestionResult::from_selection(result, |p| p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_question_enforces_shared_hostname_rules() {
        let question = hostname_question();
        assert!(question.validate(&InstallContext::new(), "archbox").is_ok());
        assert!(
            question
                .validate(&InstallContext::new(), "my_host")
                .is_err()
        );
        assert!(question.validate(&InstallContext::new(), "-lead").is_err());
        assert!(question.validate(&InstallContext::new(), "").is_err());
    }

    #[test]
    fn username_question_enforces_shared_username_rules() {
        let question = username_question();
        assert!(question.validate(&InstallContext::new(), "ben").is_ok());
        assert!(question.validate(&InstallContext::new(), "9lives").is_err());
        assert!(question.validate(&InstallContext::new(), "").is_err());
        assert_eq!(
            question
                .validate(&InstallContext::new(), "root")
                .map(|_| ()),
            Err("Username cannot be 'root'.".to_string())
        );
    }
}
