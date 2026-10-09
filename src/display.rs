//! Quick display layouts. Backend-specific discovery and application live behind
//! DisplayProvider so additional compositors can implement the same presets.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::process::Command;

use crate::common::{compositor::CompositorType, display_server::DisplayServer, instantwmctl};
use crate::menu::{
    instantmenu::InstantmenuBackend,
    protocol::{ChoiceOptions, SerializableMenuItem},
};
use crate::menu_utils::{DialogOutcome, FzfPreview};

#[derive(Debug, Clone, Deserialize)]
struct Output {
    name: String,
    modes: Vec<Mode>,
}

#[derive(Debug, Clone, Deserialize)]
struct Mode {
    width: u32,
    height: u32,
    refresh_mhz: u32,
}

#[derive(Debug, Clone, PartialEq)]
enum Layout {
    Mirror,
    Extend,
    Only(String),
}

trait DisplayProvider {
    fn outputs(&self) -> Result<Vec<Output>>;
    fn apply(&self, outputs: &[Output], layout: &Layout) -> Result<()>;
}

struct InstantWM;

impl DisplayProvider for InstantWM {
    fn outputs(&self) -> Result<Vec<Output>> {
        let mut outputs: Vec<Output> = instantwmctl::json(["monitor", "modes", "all"])?;
        outputs.retain(|output| !output.modes.is_empty());
        // Keep the internal panel first and use stable connector ordering.
        outputs.sort_by_key(|output| {
            (
                !output.name.starts_with("eDP") && !output.name.starts_with("LVDS"),
                output.name.clone(),
            )
        });
        Ok(outputs)
    }

    fn apply(&self, outputs: &[Output], layout: &Layout) -> Result<()> {
        let mirror_outputs;
        let outputs = if matches!(layout, Layout::Mirror) && DisplayServer::detect().is_x11() {
            mirror_outputs = x11_mirror_outputs(outputs)?;
            &mirror_outputs
        } else {
            outputs
        };
        let commands = layout_commands(outputs, layout)?;
        for args in commands {
            instantwmctl::run(&args).context("Failed to apply display layout")?;
        }
        Ok(())
    }
}

// RandR cannot scale mirror heads independently. Use the largest shared
// resolution and let each head pick its best refresh rate at that size.
fn x11_mirror_outputs(outputs: &[Output]) -> Result<Vec<Output>> {
    let source = outputs.first().context("No connected displays")?;
    let resolution = source.modes.iter()
        .filter(|mode| outputs.iter().all(|output| output.modes.iter().any(|candidate|
            candidate.width == mode.width && candidate.height == mode.height)))
        .max_by_key(|mode| u64::from(mode.width) * u64::from(mode.height))
        .map(|mode| (mode.width, mode.height))
        .context("Displays have no shared resolution for X11 mirroring; use Other… to configure them manually")?;
    Ok(outputs
        .iter()
        .cloned()
        .map(|mut output| {
            output
                .modes
                .retain(|mode| (mode.width, mode.height) == resolution);
            output
        })
        .collect())
}

// Enable the source first and disable unwanted heads last, so switching from
// one single-output preset to another never deliberately disables every head.
fn layout_commands(outputs: &[Output], layout: &Layout) -> Result<Vec<Vec<String>>> {
    let source = match layout {
        Layout::Only(name) => outputs.iter().find(|output| &output.name == name),
        _ => outputs.first(),
    }
    .context("No connected display matches the requested layout")?;
    let mut commands = Vec::new();
    let mut x = 0u64;
    for output in
        std::iter::once(source).chain(outputs.iter().filter(|output| output.name != source.name))
    {
        let enabled = !matches!(layout, Layout::Only(_)) || output.name == source.name;
        if !enabled {
            continue;
        }
        let mode = output
            .modes
            .iter()
            .max_by_key(|mode| {
                (
                    u64::from(mode.width) * u64::from(mode.height),
                    mode.refresh_mhz,
                )
            })
            .context("Connected display has no available modes")?;
        let mirror = matches!(layout, Layout::Mirror) && output.name != source.name;
        let mut args = vec![
            "monitor".into(),
            "set".into(),
            output.name.clone(),
            "--enable".into(),
            "true".into(),
            "--mirror".into(),
            if mirror {
                source.name.clone()
            } else {
                "none".into()
            },
            "--resolution".into(),
            format!("{}x{}", mode.width, mode.height),
            "--refresh-rate".into(),
            format!("{:.3}", f64::from(mode.refresh_mhz) / 1000.0),
            "--transform".into(),
            "normal".into(),
        ];
        if !mirror {
            args.extend([
                "--scale".into(),
                "1".into(),
                "--position".into(),
                format!("{x},0"),
            ]);
            x += u64::from(mode.width);
        }
        commands.push(args);
    }
    if matches!(layout, Layout::Only(_)) {
        for output in outputs.iter().filter(|output| output.name != source.name) {
            commands.push(vec![
                "monitor".into(),
                "set".into(),
                output.name.clone(),
                "--mirror".into(),
                "none".into(),
                "--enable".into(),
                "false".into(),
            ]);
        }
    }
    Ok(commands)
}

pub fn run() -> Result<()> {
    ensure!(
        CompositorType::detect() == CompositorType::InstantWM,
        "The display menu currently supports instantWM only"
    );
    let provider = InstantWM;
    let outputs = provider.outputs()?;
    let mut choices = Vec::new();
    if outputs.len() > 1 {
        choices.extend([
            ("Mirror".to_string(), Some(Layout::Mirror)),
            ("Extend".to_string(), Some(Layout::Extend)),
        ]);
    }
    choices.extend(outputs.iter().map(|output| {
        (
            format!("Only display {}", output.name),
            Some(Layout::Only(output.name.clone())),
        )
    }));
    choices.push(("Other…".into(), None));
    let items: Vec<_> = choices
        .iter()
        .map(|(label, _)| SerializableMenuItem {
            key: None,
            display_text: label.clone(),
            preview: FzfPreview::None,
            metadata: None,
        })
        .collect();
    let DialogOutcome::Submitted(selected) =
        InstantmenuBackend::choice(&ChoiceOptions::new("Display layout"), &items)?
    else {
        return Ok(());
    };
    let label = selected.first().context("No display layout selected")?;
    let (_, layout) = choices
        .iter()
        .find(|(candidate, _)| candidate == label)
        .context("Unknown display layout")?;
    if let Some(layout) = layout {
        // Refresh after the menu closes to avoid applying to unplugged outputs.
        let current = provider.outputs()?;
        provider.apply(&current, layout)
    } else {
        let program = if DisplayServer::detect().is_wayland() {
            "wdisplays"
        } else {
            "arandr"
        };
        let status = Command::new(program)
            .status()
            .with_context(|| format!("Failed to open {program}"))?;
        if !status.success() {
            bail!("{program} exited with {status}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn outputs() -> Vec<Output> {
        serde_json::from_str(r#"[{"name":"eDP-1","modes":[{"width":1920,"height":1080,"refresh_mhz":60000}]},{"name":"DP-1","modes":[{"width":2560,"height":1440,"refresh_mhz":144000}]}]"#).unwrap()
    }
    #[test]
    fn extend_clears_mirrors_and_places_heads_side_by_side() {
        let commands = layout_commands(&outputs(), &Layout::Extend).unwrap();
        assert!(
            commands[0]
                .windows(2)
                .any(|pair| pair == ["--position", "0,0"])
        );
        assert!(
            commands[1]
                .windows(2)
                .any(|pair| pair == ["--position", "1920,0"])
        );
        assert!(
            commands
                .iter()
                .all(|args| args.windows(2).any(|pair| pair == ["--mirror", "none"]))
        );
    }
    #[test]
    fn x11_mirror_requires_and_selects_a_shared_resolution() {
        let mut displays = outputs();
        assert!(x11_mirror_outputs(&displays).is_err());
        displays[1].modes.push(Mode {
            width: 1920,
            height: 1080,
            refresh_mhz: 60000,
        });
        let shared = x11_mirror_outputs(&displays).unwrap();
        let commands = layout_commands(&shared, &Layout::Mirror).unwrap();
        assert!(commands.iter().all(|args| {
            args.windows(2)
                .any(|pair| pair == ["--resolution", "1920x1080"])
        }));
    }

    #[test]
    fn mirror_uses_first_head_as_source() {
        let commands = layout_commands(&outputs(), &Layout::Mirror).unwrap();
        assert!(
            commands[1]
                .windows(2)
                .any(|pair| pair == ["--mirror", "eDP-1"])
        );
        assert!(!commands[1].iter().any(|arg| arg == "--position"));
    }
    #[test]
    fn only_enables_target_before_disabling_others() {
        let commands = layout_commands(&outputs(), &Layout::Only("DP-1".into())).unwrap();
        assert_eq!(commands[0][2], "DP-1");
        assert!(
            commands[0]
                .windows(2)
                .any(|pair| pair == ["--enable", "true"])
        );
        assert_eq!(commands[1][2], "eDP-1");
        assert!(
            commands[1]
                .windows(2)
                .any(|pair| pair == ["--enable", "false"])
        );
        assert!(layout_commands(&outputs(), &Layout::Only("missing".into())).is_err());
    }
}
