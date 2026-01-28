use anyhow::Result;

use crate::settings::store::{IntSettingKey, SettingsStore};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

const MOUSE_SENSITIVITY_KEY: IntSettingKey = IntSettingKey::new("desktop.mouse.sensitivity", 50);

pub(crate) fn render_mouse_sensitivity_preview() -> Result<String> {
    let store = SettingsStore::load()?;
    let current = store.int(MOUSE_SENSITIVITY_KEY);

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
