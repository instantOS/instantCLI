use crate::common::systemd::SystemdManager;
use crate::settings::constants::PACCACHE_TIMER_UNIT;
use crate::settings::store::PACMAN_AUTOCLEAN_KEY;
use crate::settings::store::BoolSettingKey;

/// Abstract source of truth for a boolean setting.
pub trait BoolStateSource: Send + Sync {
    fn current(&self) -> anyhow::Result<bool>;
    fn apply(&self, desired: bool) -> anyhow::Result<()>;
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

static BOOL_SOURCES: [(&BoolSettingKey, &'static dyn BoolStateSource); 1] =
    [(&PACMAN_AUTOCLEAN_KEY, &PACCACHE_SOURCE)];

pub fn source_for(key: &BoolSettingKey) -> Option<&'static dyn BoolStateSource> {
    BOOL_SOURCES
        .iter()
        .find(|(candidate, _)| candidate.key == key.key)
        .map(|(_, source)| *source)
}

pub fn all_bool_sources() -> &'static [(&'static BoolSettingKey, &'static dyn BoolStateSource)] {
    &BOOL_SOURCES
}
