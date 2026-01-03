//! Qt settings
//!
//! Reset Qt theme customizations (qt5ct, qt6ct, Kvantum).

use anyhow::{Context, Result};

use crate::menu_utils::FzfWrapper;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

pub struct ResetQt;

impl Setting for ResetQt {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("appearance.reset_qt")
            .title("Reset Customizations")
            .icon(NerdFont::Trash)
            .summary("Reset all Qt theme settings to default.\n\nRemoves qt5ct, qt6ct, and Kvantum configuration directories.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let confirmation = FzfWrapper::confirm(
            "Are you sure you want to reset all Qt theme customizations? This will remove qt5ct, qt6ct, and Kvantum configurations.",
        )?;

        if matches!(confirmation, crate::menu_utils::ConfirmResult::Yes)
            && let Ok(config_dir) = dirs::config_dir().context("Could not find config directory")
        {
            let dirs_to_remove = [
                config_dir.join("qt5ct"),
                config_dir.join("qt6ct"),
                config_dir.join("Kvantum"),
            ];

            let mut removed_count = 0;
            for dir in &dirs_to_remove {
                if dir.exists()
                    && let Ok(()) = std::fs::remove_dir_all(dir)
                {
                    removed_count += 1;
                }
            }

            if removed_count > 0 {
                ctx.notify(
                    "Qt Reset",
                    &format!(
                        "Removed {} Qt configuration {}. Restart Qt applications to see changes.",
                        removed_count,
                        if removed_count == 1 {
                            "directory"
                        } else {
                            "directories"
                        }
                    ),
                );
            } else {
                ctx.notify(
                    "Qt Reset",
                    "No Qt configuration directories found to remove.",
                );
            }
        }

        Ok(())
    }
}
