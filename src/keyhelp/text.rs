use crate::common::compositor::CompositorType;
use anyhow::Result;

use super::fetch::{fetch_hyprland_keybinds, fetch_keybinds};

/// Non-tty fallback: print a plain-text table, like `ins assist list`.
pub(super) fn print_text_list() -> Result<()> {
    let compositor = CompositorType::detect();
    let rows = match compositor {
        CompositorType::InstantWM => fetch_keybinds()?,
        CompositorType::Hyprland => fetch_hyprland_keybinds()?,
        _ => {
            eprintln!(
                "ins keyhelp: keybind listing is only supported on instantWM and Hyprland \
                 (detected: {}).",
                compositor.name()
            );
            return Ok(());
        }
    };
    if rows.is_empty() {
        if matches!(compositor, CompositorType::Hyprland) {
            println!("No keybindings found. Is Hyprland running?");
        } else {
            println!("No keybindings found. Is instantWM running?");
        }
        return Ok(());
    }

    let bind_w = rows.iter().map(|r| r.binding().len()).max().unwrap_or(0);
    let act_w = rows.iter().map(|r| r.action.len()).max().unwrap_or(0);

    println!(
        "{:<bw$} | {:<aw$} | MODE | ORIGIN",
        "BINDING",
        "ACTION",
        bw = bind_w,
        aw = act_w
    );
    println!(
        "{:-<bw$}-|-{:-<aw$}-|-----|------",
        "",
        "",
        bw = bind_w,
        aw = act_w
    );
    for r in &rows {
        let origin = if r.origin == "user" {
            "config"
        } else if r.origin.starts_with("hyprland") {
            r.origin.as_str()
        } else {
            "default"
        };
        println!(
            "{:<bw$} | {:<aw$} | {:5} | {:7}",
            r.binding(),
            r.action,
            r.mode,
            origin,
            bw = bind_w,
            aw = act_w
        );
    }
    Ok(())
}
