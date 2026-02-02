//! Storage settings
//!
//! Auto-mount, disk management, and partition editor.

use anyhow::Result;

use crate::common::package::InstallResult;
use crate::common::systemd::{SystemdManager, UserServiceConfig};
use crate::settings::context::SettingsContext;
use crate::settings::deps::{GNOME_DISKS, GPARTED, UDISKIE};
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::BoolSettingKey;
use crate::ui::prelude::*;

// ============================================================================
// Auto-mount Disks
// ============================================================================

pub struct AutomountDisks;

impl AutomountDisks {
    const KEY: BoolSettingKey = BoolSettingKey::new("storage.udiskie", false);
}

impl Setting for AutomountDisks {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("storage.automount")
            .title("Auto-mount disks")
            .icon(NerdFont::HardDrive)
            .summary("Automatically mount removable drives with udiskie.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);

        const UDISKIE_SERVICE_NAME: &str = "udiskie";

        // Ensure udiskie is installed
        match UDISKIE.ensure()? {
            InstallResult::Installed | InstallResult::AlreadyInstalled => {}
            _ => {
                ctx.set_bool(Self::KEY, false);
                ctx.emit_info(
                    "settings.storage.udiskie.aborted",
                    "Auto-mount setup was cancelled.",
                );
                return Ok(());
            }
        }

        let systemd_manager = SystemdManager::user();

        if enabled {
            let service_config = UserServiceConfig::new(
                UDISKIE_SERVICE_NAME,
                "udiskie removable media automounter",
                "/usr/bin/udiskie",
            );

            if let Err(err) = systemd_manager.create_user_service(&service_config) {
                emit(
                    Level::Warn,
                    "settings.storage.udiskie.service_creation_failed",
                    &format!(
                        "{} Failed to create udiskie service file: {err}",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
                return Err(err);
            }

            if !systemd_manager.is_enabled(UDISKIE_SERVICE_NAME) {
                systemd_manager.enable_and_start(UDISKIE_SERVICE_NAME)?;
            } else if !systemd_manager.is_active(UDISKIE_SERVICE_NAME) {
                systemd_manager.start(UDISKIE_SERVICE_NAME)?;
            }

            ctx.notify(
                "Auto-mount",
                "udiskie service enabled - removable drives will auto-mount",
            );
        } else if systemd_manager.is_enabled(UDISKIE_SERVICE_NAME)
            || systemd_manager.is_active(UDISKIE_SERVICE_NAME)
        {
            systemd_manager.disable_and_stop(UDISKIE_SERVICE_NAME)?;
            ctx.notify("Auto-mount", "udiskie service disabled");
        }

        Ok(())
    }

    // No restore needed - systemd handles service persistence
}

gui_command_setting!(
    DiskManagement,
    "storage.disks",
    "Disk management",
    NerdFont::HardDrive,
    "Launch GNOME Disks to manage drives and partitions.",
    "gnome-disks",
    &GNOME_DISKS
);

gui_command_setting!(
    PartitionEditor,
    "storage.gparted",
    "Partition editor",
    NerdFont::Partition,
    "Launch GParted for advanced partition management (requires root).",
    "gparted",
    &GPARTED
);
