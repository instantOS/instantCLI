use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::Command;

use crate::common::compositor::CompositorType;
use crate::common::display::{OutputInfo, SwayDisplayProvider};
use crate::menu::client::MenuClient;
use crate::menu::protocol::{FzfPreview, SerializableMenuItem};

pub fn mirror_output() -> Result<()> {
    let compositor = CompositorType::detect();
    if !matches!(compositor, CompositorType::Sway) {
        anyhow::bail!(
            "wl-mirror assist currently supports Sway only. Detected: {}.",
            compositor.name()
        );
    }

    let outputs =
        SwayDisplayProvider::get_outputs_sync().context("Failed to query Sway outputs")?;

    if outputs.is_empty() {
        crate::assist::utils::show_notification("wl-mirror", "No outputs detected")?;
        return Ok(());
    }

    let output_name = if outputs.len() == 1 {
        outputs[0].name.clone()
    } else {
        match select_output(&outputs)? {
            Some(name) => name,
            None => return Ok(()),
        }
    };

    Command::new("wl-mirror")
        .arg(&output_name)
        .spawn()
        .context("Failed to launch wl-mirror")?;

    Ok(())
}

fn select_output(outputs: &[OutputInfo]) -> Result<Option<String>> {
    let items: Vec<SerializableMenuItem> = outputs
        .iter()
        .map(|output| {
            let mut metadata = HashMap::new();
            metadata.insert("output".to_string(), output.name.clone());
            SerializableMenuItem {
                display_text: output.display_label(),
                preview: FzfPreview::None,
                metadata: Some(metadata),
            }
        })
        .collect();

    let client = MenuClient::new();
    let selected = client.choice("Mirror which output?".to_string(), items, false)?;

    let item = match selected.first() {
        Some(item) => item,
        None => return Ok(None),
    };

    let output_name = item
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("output").cloned())
        .or_else(|| {
            item.display_text
                .split_whitespace()
                .next()
                .map(|s| s.to_string())
        });

    Ok(output_name)
}
