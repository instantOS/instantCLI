//! Printer settings
//!
//! Printer services and management.

use anyhow::Result;

use crate::settings::context::SettingsContext;
use crate::settings::printer;
use crate::settings::setting::{Category, Requirement, Setting, SettingMetadata, SettingType};
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
        SettingMetadata {
            id: "printers.enable_services",
            title: "Printer services",
            category: Category::Printers,
            icon: NerdFont::Printer,
            icon_color: None,
            breadcrumbs: &["Printer support", "Services"],
            summary: "Enable CUPS printing and Avahi discovery for network printers.",
            requires_reapply: false,
            requirements: &[
                Requirement::Package(printer::CUPS_PACKAGE),
                Requirement::Package(printer::AVAHI_PACKAGE),
                Requirement::Package(printer::CUPS_FILTERS_PACKAGE),
                Requirement::Package(printer::GHOSTSCRIPT_PACKAGE),
                Requirement::Package(printer::NSS_MDNS_PACKAGE),
            ],
        }
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

inventory::submit! { &PrinterServices as &'static dyn Setting }

// ============================================================================
// Open Printer Manager
// ============================================================================

pub struct PrinterManager;

impl Setting for PrinterManager {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata {
            id: "printers.open_manager",
            title: "Open printer manager",
            category: Category::Printers,
            icon: NerdFont::Printer,
            icon_color: None,
            breadcrumbs: &["Printer support", "Manage printers"],
            summary: "Launch the graphical printer setup utility.",
            requires_reapply: false,
            requirements: &[Requirement::Package(printer::SYSTEM_CONFIG_PRINTER_PACKAGE)],
        }
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        printer::launch_printer_manager(ctx)
    }
}

inventory::submit! { &PrinterManager as &'static dyn Setting }
