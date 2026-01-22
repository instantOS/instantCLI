use crate::common::systemd::SystemdManager;
use crate::settings::constants::PACCACHE_TIMER_UNIT;
use crate::settings::definitions::appearance::common::{
    apply_gtk4_overrides, get_current_gtk_theme, get_current_icon_theme, set_gtk_theme,
    set_icon_theme, update_gtk_config,
};
use crate::settings::store::{
    BoolSettingKey, GTK_ICON_THEME_KEY, GTK_THEME_KEY, PACMAN_AUTOCLEAN_KEY, StringSettingKey,
};

/// Abstract source of truth for a boolean setting.
pub trait BoolStateSource: Send + Sync {
    fn current(&self) -> anyhow::Result<bool>;
    fn apply(&self, desired: bool) -> anyhow::Result<()>;
}

/// Abstract source of truth for a string setting.
pub trait StringStateSource: Send + Sync {
    fn current(&self) -> anyhow::Result<String>;
    fn apply(&self, desired: &str) -> anyhow::Result<()>;
}

/// Systemd-backed boolean source gating a unit's enablement.
pub struct SystemdUnitSource {
    unit: &'static str,
}

impl SystemdUnitSource {
    pub const fn new(unit: &'static str) -> Self {
        Self { unit }
    }
}

impl BoolStateSource for SystemdUnitSource {
    fn current(&self) -> anyhow::Result<bool> {
        Ok(SystemdManager::system().is_enabled(self.unit))
    }

    fn apply(&self, desired: bool) -> anyhow::Result<()> {
        let manager = SystemdManager::system_with_sudo();
        if desired {
            manager.enable_and_start(self.unit)?;
        } else if manager.is_enabled(self.unit) || manager.is_active(self.unit) {
            manager.disable_and_stop(self.unit)?;
        }
        Ok(())
    }
}

static PACCACHE_SOURCE: SystemdUnitSource = SystemdUnitSource::new(PACCACHE_TIMER_UNIT);

struct GtkThemeSource;

impl StringStateSource for GtkThemeSource {
    fn current(&self) -> anyhow::Result<String> {
        get_current_gtk_theme()
    }

    fn apply(&self, desired: &str) -> anyhow::Result<()> {
        set_gtk_theme(desired)?;
        let _ = update_gtk_config("3.0", "gtk-theme-name", desired);
        let _ = update_gtk_config("4.0", "gtk-theme-name", desired);
        let _ = apply_gtk4_overrides(desired);
        Ok(())
    }
}

struct GtkIconThemeSource;

impl StringStateSource for GtkIconThemeSource {
    fn current(&self) -> anyhow::Result<String> {
        get_current_icon_theme()
    }

    fn apply(&self, desired: &str) -> anyhow::Result<()> {
        set_icon_theme(desired)?;
        let _ = update_gtk_config("3.0", "gtk-icon-theme-name", desired);
        let _ = update_gtk_config("4.0", "gtk-icon-theme-name", desired);
        Ok(())
    }
}

static GTK_THEME_SOURCE: GtkThemeSource = GtkThemeSource;
static GTK_ICON_THEME_SOURCE: GtkIconThemeSource = GtkIconThemeSource;

static BOOL_SOURCES: [(&BoolSettingKey, &'static dyn BoolStateSource); 1] =
    [(&PACMAN_AUTOCLEAN_KEY, &PACCACHE_SOURCE)];

static STRING_SOURCES: [(&StringSettingKey, &'static dyn StringStateSource); 2] = [
    (&GTK_THEME_KEY, &GTK_THEME_SOURCE),
    (&GTK_ICON_THEME_KEY, &GTK_ICON_THEME_SOURCE),
];

pub fn source_for(key: &BoolSettingKey) -> Option<&'static dyn BoolStateSource> {
    BOOL_SOURCES
        .iter()
        .find(|(candidate, _)| candidate.key == key.key)
        .map(|(_, source)| *source)
}

pub fn all_bool_sources() -> &'static [(&'static BoolSettingKey, &'static dyn BoolStateSource)] {
    &BOOL_SOURCES
}

pub fn string_source_for(key: &StringSettingKey) -> Option<&'static dyn StringStateSource> {
    STRING_SOURCES
        .iter()
        .find(|(candidate, _)| candidate.key == key.key)
        .map(|(_, source)| *source)
}

pub fn all_string_sources() -> &'static [(&'static StringSettingKey, &'static dyn StringStateSource)]
{
    &STRING_SOURCES
}
