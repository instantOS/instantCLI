use anyhow::Result;

use crate::common::compositor::CompositorType;
use crate::settings::store::{IntSettingKey, SettingsStore};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

const MOUSE_SENSITIVITY_KEY: IntSettingKey = IntSettingKey::new("desktop.mouse.sensitivity", 50);

pub(crate) fn render_mouse_sensitivity_preview() -> Result<String> {
    let store = SettingsStore::load()?;

    // Get current value: use stored value if present, otherwise detect from system
    let current = if store.contains(MOUSE_SENSITIVITY_KEY.key) {
        store.int(MOUSE_SENSITIVITY_KEY)
    } else {
        // Detect current speed based on compositor
        let compositor = CompositorType::detect();
        let speed = match compositor {
            CompositorType::Sway => {
                crate::assist::actions::mouse::get_sway_mouse_speed().unwrap_or(0.0)
            }
            CompositorType::Gnome => {
                crate::assist::actions::mouse::get_gnome_mouse_speed().unwrap_or(0.0)
            }
            _ if compositor.is_x11() => {
                crate::assist::actions::mouse::get_x11_mouse_speed().unwrap_or(0.0)
            }
            _ => 0.0,
        };
        // Map -1.0..1.0 to 0..100
        ((speed + 1.0) * 50.0) as i64
    };

    let display_value = if store.contains(MOUSE_SENSITIVITY_KEY.key) {
        current.to_string()
    } else {
        format!("{} (default)", current)
    };

    Ok(PreviewBuilder::new()
        .header(NerdFont::Mouse, "Mouse Sensitivity")
        .text("Adjust mouse pointer speed using an interactive slider.")
        .text("The setting will be automatically restored on login.")
        .blank()
        .field("Current sensitivity", &display_value)
        .build_string())
}
