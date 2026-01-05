//! Display settings
//!
//! Monitor resolution, refresh rate, and display configuration.

use anyhow::Result;

use crate::common::compositor::CompositorType;
use crate::common::display::SwayDisplayProvider;
use crate::menu_utils::FzfWrapper;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::prelude::*;

// ============================================================================
// Display Configuration
// ============================================================================

pub struct ConfigureDisplay;

impl Setting for ConfigureDisplay {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("display.configure")
            .title("Display Configuration")
            .icon(NerdFont::Monitor)
            .summary("Configure display resolution and refresh rate.\n\nSelect a display and choose from available modes.\n\nCurrently only supported on Sway.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let compositor = CompositorType::detect();
        if !matches!(compositor, CompositorType::Sway) {
            ctx.emit_unsupported(
                "settings.display.configure.unsupported",
                &format!(
                    "Display configuration is only supported on Sway. Detected: {}.",
                    compositor.name()
                ),
            );
            return Ok(());
        }

        // Get outputs
        let outputs = match SwayDisplayProvider::get_outputs_sync() {
            Ok(outputs) => outputs,
            Err(e) => {
                ctx.emit_failure(
                    "settings.display.configure.query_failed",
                    &format!("Failed to query displays: {e}"),
                );
                return Ok(());
            }
        };

        if outputs.is_empty() {
            ctx.emit_info(
                "settings.display.configure.no_displays",
                "No displays detected.",
            );
            return Ok(());
        }

        // If there's only one display, use it directly without prompting
        let output = if outputs.len() == 1 {
            outputs.first().unwrap()
        } else {
            // Build display selection menu
            let display_options: Vec<String> = outputs.iter().map(|o| o.display_label()).collect();

            let selected_display = FzfWrapper::builder()
                .prompt("Select Display")
                .header("Choose a display to configure")
                .select(display_options.clone())?;

            match selected_display {
                crate::menu_utils::FzfResult::Selected(selection) => {
                    outputs.iter().find(|o| o.display_label() == selection)
                }
                _ => return Ok(()),
            }
            .ok_or_else(|| anyhow::anyhow!("No display selected"))?
        };

        // Build resolution/refresh rate menu
        // Sort modes: highest resolution first, then highest refresh rate
        let mut sorted_modes = output.available_modes.clone();
        sorted_modes.sort_by(|a, b| {
            b.resolution()
                .cmp(&a.resolution())
                .then(b.refresh.cmp(&a.refresh))
        });

        // Build menu options with current mode marked
        let mode_options: Vec<String> = sorted_modes
            .iter()
            .map(|m| {
                let label = m.display_format();
                if *m == output.current_mode {
                    format!("{} (current)", label)
                } else {
                    label
                }
            })
            .collect();

        let selected_mode = FzfWrapper::builder()
            .prompt("Select Mode")
            .header(format!("Choose resolution/refresh for {}", output.name))
            .select(mode_options)?;

        let target_mode = match selected_mode {
            crate::menu_utils::FzfResult::Selected(selection) => {
                // Strip " (current)" suffix if present
                let clean_selection = selection.trim_end_matches(" (current)");
                sorted_modes
                    .iter()
                    .find(|m| m.display_format() == clean_selection)
            }
            _ => return Ok(()),
        };

        let mode = match target_mode {
            Some(m) => m,
            None => return Ok(()),
        };

        // Apply the mode
        if let Err(e) = SwayDisplayProvider::set_output_mode_sync(&output.name, mode) {
            ctx.emit_failure(
                "settings.display.configure.apply_failed",
                &format!("Failed to apply mode: {e}"),
            );
            return Ok(());
        }

        ctx.notify(
            "Display",
            &format!("{} set to {}", output.name, mode.display_format()),
        );
        Ok(())
    }
}
