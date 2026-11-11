//! `ins keyhelp` - explore and memorize instantWM keybinds.
//!
//! Fetches the live binding list over `instantwmctl --json keybinds` and
//! presents it as a searchable fzf menu. Each binding shows a rich preview
//! (mode availability, origin, and the built-in action description from
//! `instantwmctl action --list`). Selecting an entry opens a small action
//! menu: run the action, open the config file in the editor, or go back to
//! the keybind list.
//!
//! The binding preview is rendered lazily by a hidden `ins preview --id
//! keyhelp` subcommand so we only build preview text for the highlighted row.
//!
//! The main menu tracks the cursor with [`MenuCursor`] so the user's place
//! is restored after returning from the per-binding action submenu (same
//! pattern as the notification center and `ins settings`).

mod fetch;
mod keywords;
mod menu;
mod model;
mod paths;
mod ranking;
mod text;

#[cfg(test)]
mod tests;

pub use fetch::fetch_action_docs;
pub use keywords::derive_keywords;
pub use model::{ActionDoc, KeybindPreviewPayload};

use crate::common::compositor::CompositorType;
use crate::menu_utils::{FzfWrapper, HeaderBuilder, MenuCursor};
use crate::ui::nerd_font::NerdFont;
use anyhow::Result;
use std::io::IsTerminal;

use fetch::{fetch_hyprland_keybinds, fetch_keybinds};
use menu::{
    SubmenuAction, handle_select, handle_select_hyprland, open_config, open_hyprland_config,
    run_binding,
};
use text::print_text_list;

/// Mode tag used for bindings that are active in every mode.
const GLOBAL_MODE: &str = "global";

/// Dispatch entry point for `ins keyhelp` with `--gui` support.
pub fn run_keyhelp_command(gui: bool, debug: bool) -> Result<()> {
    if gui {
        return launch_keyhelp_in_terminal(debug);
    }
    run_keyhelp()
}

/// Launch `ins keyhelp` in a dedicated GUI terminal window.
pub fn launch_keyhelp_in_terminal(debug: bool) -> Result<()> {
    let mut args: Vec<String> = vec![];

    if debug {
        args.push("--debug".to_string());
    }

    args.push("keyhelp".to_string());

    let current_exe = std::env::current_exe()?;
    instantos_terminal::Launcher::new(current_exe.as_os_str())
        .app_id("ins-keyhelp")
        .title("Keyhelp")
        .args(&args)
        .spawn_detached()
        .map(|_| ())
        .map_err(Into::into)
}

/// Entry point for `ins keyhelp`.
pub fn run_keyhelp() -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return print_text_list();
    }

    let compositor = CompositorType::detect();
    let (rows, is_hyprland) = match compositor {
        CompositorType::InstantWM => (fetch_keybinds()?, false),
        CompositorType::Hyprland => (fetch_hyprland_keybinds()?, true),
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
        if is_hyprland {
            println!("No keybindings found. Is Hyprland running?");
        } else {
            println!("No keybindings found. Is instantWM running?");
        }
        return Ok(());
    }

    // Track the user's place across the action submenu, same pattern as
    // `ins settings` and the notification center. After they pick an
    // action (or hit Back), the next open of the keybind list lands on
    // the same row.
    let mut cursor = MenuCursor::new();
    let header_title = if is_hyprland {
        format!("Hyprland keybinds  ·  {} bindings", rows.len())
    } else {
        format!("instantWM keybinds  ·  {} bindings", rows.len())
    };
    let header_subtitle = if is_hyprland {
        "Select a binding to view it or edit hyprland.lua (descriptions show if set)"
    } else {
        "Select a binding to run it, edit its config, or learn about it"
    };

    loop {
        let header = HeaderBuilder::new(NerdFont::Keyboard, header_title.clone())
            .subtitle(header_subtitle)
            .build();

        let selection = FzfWrapper::builder()
            .prompt(format!("{} ", char::from(NerdFont::Search)))
            .header(header.clone())
            .responsive_layout()
            .cursor(cursor.initial_index(&rows))
            .items(rows.clone())
            .padded()
            .select_one()?;

        match selection {
            crate::menu_utils::DialogOutcome::Submitted(row) => {
                cursor.update(&row, &rows);
                // Hyprland binds are Lua closures (dispatcher __lua). They can't be
                // triggered via instantwmctl. Offer view/edit only, and show a hint
                // for undocumented binds.
                if is_hyprland {
                    match handle_select_hyprland(&row)? {
                        SubmenuAction::Edit => open_hyprland_config()?,
                        SubmenuAction::Back => continue,
                        SubmenuAction::Run => {
                            crate::menu_utils::FzfWrapper::message(
                                "Hyprland binds can't be triggered from keyhelp. Open hyprland.lua to run or test the dispatcher manually.",
                            )?;
                        }
                    }
                } else {
                    match handle_select(&row)? {
                        SubmenuAction::Run => run_binding(&row)?,
                        SubmenuAction::Edit => open_config()?,
                        SubmenuAction::Back => continue,
                    }
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(()),
        }
    }
}
