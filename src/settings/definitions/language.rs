//! Language & region settings
//!
//! System language/locale configuration and timezone.

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::setting::{Category, Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// System Language
// ============================================================================

pub struct SystemLanguage;

impl Setting for SystemLanguage {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.main")
            .title("Language")
            .category(Category::Language)
            .icon(NerdFont::Globe)
            .breadcrumbs(&["Language"])
            .summary("Manage system locales and choose the default language.\n\nEnable or disable locales in /etc/locale.gen and set LANG via localectl.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        crate::settings::language::configure_system_language(ctx)
    }
}

inventory::submit! { &SystemLanguage as &'static dyn Setting }

// ============================================================================
// Timezone
// ============================================================================

pub struct Timezone;

impl Setting for Timezone {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("system.timezone")
            .title("Timezone")
            .category(Category::Language)
            .icon(NerdFont::Clock)
            .breadcrumbs(&["Language", "Timezone"])
            .summary("Select the system timezone via timedatectl set-timezone.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        crate::settings::actions::configure_timezone(ctx)
    }
}

inventory::submit! { &Timezone as &'static dyn Setting }
