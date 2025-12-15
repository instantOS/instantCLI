//! Printer settings
//!
//! Printer services and management.

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::deps::{
    AVAHI, CUPS, CUPS_FILTERS, GHOSTSCRIPT, NSS_MDNS, SYSTEM_CONFIG_PRINTER,
};
use crate::settings::printer;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::BoolSettingKey;
use crate::ui::prelude::*;

// ============================================================================
// Printer Services
// ============================================================================

pub struct PrinterServices;

impl PrinterServices {
    const KEY: BoolSettingKey = BoolSettingKey::new("printers.services", false);
}

impl Setting for PrinterServices {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("printers.enable_services")
            .title("Printer services")
            .icon(NerdFont::Printer)
            .summary("Enable CUPS printing and Avahi discovery for network printers.")
            .requirements(vec![&CUPS, &AVAHI, &CUPS_FILTERS, &GHOSTSCRIPT, &NSS_MDNS])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let target = !current;
        ctx.set_bool(Self::KEY, target);
        printer::configure_printer_support(ctx, target)
    }
}

// ============================================================================
// Open Printer Manager
// ============================================================================

pub struct PrinterManager;

impl Setting for PrinterManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("printers.open_manager")
            .title("Open printer manager")
            .icon(NerdFont::Printer)
            .summary("Launch the graphical printer setup utility.")
            .requirements(vec![&SYSTEM_CONFIG_PRINTER])
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        printer::launch_printer_manager(ctx)
    }
}
