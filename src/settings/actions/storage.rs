//! Storage-related settings actions

use anyhow::Result;

use crate::common::requirements::UDISKIE_PACKAGE;
use crate::common::systemd::{SystemdManager, UserServiceConfig};
use crate::ui::prelude::*;

use super::super::context::SettingsContext;
use super::super::store::UDISKIE_AUTOMOUNT_KEY;

const UDISKIE_SERVICE_NAME: &str = "udiskie";

pub fn apply_udiskie_automount(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    // Ensure udiskie is installed
    if !UDISKIE_PACKAGE.ensure()? {
        ctx.set_bool(UDISKIE_AUTOMOUNT_KEY, false);
        ctx.emit_info(
            "settings.storage.udiskie.aborted",
            "Auto-mount setup was cancelled.",
        );
        return Ok(());
    }

    let systemd_manager = SystemdManager::user();

    if enabled {
        // Create the udiskie service configuration
        let service_config = UserServiceConfig::new(
            UDISKIE_SERVICE_NAME,
            "udiskie removable media automounter",
            "/usr/bin/udiskie",
        );

        // Create the user service file
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

        // Enable and start the service
        if !systemd_manager.is_enabled(UDISKIE_SERVICE_NAME) {
            if let Err(err) = systemd_manager.enable_and_start(UDISKIE_SERVICE_NAME) {
                emit(
                    Level::Warn,
                    "settings.storage.udiskie.enable_failed",
                    &format!(
                        "{} Failed to enable udiskie service: {err}",
                        char::from(NerdFont::Warning)
                    ),
                    None,
                );
                return Err(err);
            }
        } else if !systemd_manager.is_active(UDISKIE_SERVICE_NAME)
            && let Err(err) = systemd_manager.start(UDISKIE_SERVICE_NAME)
        {
            emit(
                Level::Warn,
                "settings.storage.udiskie.start_failed",
                &format!(
                    "{} Failed to start udiskie service: {err}",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            return Err(err);
        }

        ctx.notify(
            "Auto-mount",
            "udiskie service enabled - removable drives will auto-mount",
        );
    } else {
        // Disable and stop the service
        if (systemd_manager.is_enabled(UDISKIE_SERVICE_NAME)
            || systemd_manager.is_active(UDISKIE_SERVICE_NAME))
            && let Err(err) = systemd_manager.disable_and_stop(UDISKIE_SERVICE_NAME)
        {
            emit(
                Level::Warn,
                "settings.storage.udiskie.disable_failed",
                &format!(
                    "{} Failed to disable udiskie service: {err}",
                    char::from(NerdFont::Warning)
                ),
                None,
            );
            return Err(err);
        }

        ctx.notify("Auto-mount", "udiskie service disabled");
    }

    Ok(())
}
