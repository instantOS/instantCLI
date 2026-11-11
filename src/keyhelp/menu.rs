use crate::common::instantwmctl;
use crate::menu_utils::{FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::Command;

use super::GLOBAL_MODE;
use super::model::{KeybindRow, action_name};
use super::paths::{config_path, hyprland_config_path};

/// Choice made in the per-binding action menu. Mirrors the systemd
/// submenu shape: a small enum carried by a thin `SubmenuActionItem`
/// wrapper that renders the row + its preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SubmenuAction {
    Run,
    Edit,
    Back,
}

/// One row in the per-binding action menu.
///
/// `run_action` carries the action string for the `Run` variant so the
/// preview can show the `instantwmctl action ...` command that will run.
/// `Edit`/`Back` don't need it.
#[derive(Debug, Clone)]
pub(super) struct SubmenuActionItem {
    action: SubmenuAction,
    run_action: Option<String>,
}

impl SubmenuActionItem {
    pub(super) fn run(action: String) -> Self {
        Self {
            action: SubmenuAction::Run,
            run_action: Some(action),
        }
    }

    pub(super) fn edit() -> Self {
        Self {
            action: SubmenuAction::Edit,
            run_action: None,
        }
    }

    pub(super) fn back() -> Self {
        Self {
            action: SubmenuAction::Back,
            run_action: None,
        }
    }
}

impl FzfSelectable for SubmenuActionItem {
    fn fzf_display_text(&self) -> String {
        match self.action {
            SubmenuAction::Run => {
                format!("{} Run", format_icon_colored(NerdFont::Play, colors::GREEN),)
            }
            SubmenuAction::Edit => format!("{} Edit config", format_icon(NerdFont::Edit),),
            SubmenuAction::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        match self.action {
            SubmenuAction::Run => {
                let action = self.run_action.as_deref().unwrap_or("");
                // Mirror `run_binding`: first token is the action name, the
                // rest are its args, joined back into the same shell call.
                let name = action_name(action);
                let args = action[name.len()..].trim_start();
                let command = if args.is_empty() {
                    format!("instantwmctl action {name}")
                } else {
                    format!("instantwmctl action {name} {args}")
                };
                crate::menu_utils::FzfPreview::Text(
                    PreviewBuilder::new()
                        .title(colors::BLUE, "Run this action")
                        .blank()
                        .text("Executes the binding's action right now.")
                        .field("Command", &command)
                        .build_string(),
                )
            }
            SubmenuAction::Edit => {
                let path = config_path();
                crate::menu_utils::FzfPreview::Text(
                    PreviewBuilder::new()
                        .title(colors::BLUE, "Edit config")
                        .blank()
                        .text(
                            "Open your instantWM config so you can change or remove this binding.",
                        )
                        .field("File", &path.display().to_string())
                        .field("Editor", "$EDITOR (nvim by default)")
                        .build_string(),
                )
            }
            SubmenuAction::Back => crate::menu_utils::FzfPreview::Text(
                PreviewBuilder::new()
                    .subtext("Return to the keybind list")
                    .build_string(),
            ),
        }
    }

    fn fzf_key(&self) -> String {
        match self.action {
            SubmenuAction::Run => "run".to_string(),
            SubmenuAction::Edit => "edit".to_string(),
            SubmenuAction::Back => "back".to_string(),
        }
    }
}

/// Let the user pick what to do with a selected binding.
pub(super) fn handle_select(row: &KeybindRow) -> Result<SubmenuAction> {
    // Sequences can't be triggered individually, so offer edit/back only.
    let mut options: Vec<SubmenuActionItem> = Vec::new();
    if row.sequence_steps().is_none() {
        options.push(SubmenuActionItem::run(row.action.clone()));
    }
    options.push(SubmenuActionItem::edit());
    options.push(SubmenuActionItem::back());

    let mut header = HeaderBuilder::new(NerdFont::Keyboard, row.binding())
        .subtitle(&row.action)
        .field("Mode", &row.mode);
    if row.mode != GLOBAL_MODE {
        header = header.field("Availability", format!("only in '{}' mode", row.mode));
    }
    header = header.field("Origin", row.origin_label());
    let header = header.build();

    let selection = FzfWrapper::builder()
        .prompt(format!("{} ", char::from(NerdFont::Wrench)))
        .header(header)
        .responsive_layout()
        .items(options)
        .padded()
        .select_one()?;

    match selection {
        crate::menu_utils::DialogOutcome::Submitted(item) => Ok(item.action),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(SubmenuAction::Back),
    }
}

pub(super) fn handle_select_hyprland(row: &KeybindRow) -> Result<SubmenuAction> {
    // Hyprland binds are Lua closures (dispatcher __lua). Can't be triggered via instantwmctl.
    // Only offer Edit config / Back, and annotate undocumented binds in the preview.
    let options: Vec<SubmenuActionItem> =
        vec![SubmenuActionItem::edit(), SubmenuActionItem::back()];

    let mut header = HeaderBuilder::new(NerdFont::Keyboard, row.binding())
        .subtitle(&row.action)
        .field("Mode", &row.mode);
    if row.mode != GLOBAL_MODE {
        header = header.field("Availability", format!("only in '{}' submap", row.mode));
    }
    header = header.field("Origin", &row.origin);
    if row.origin == "hyprland.lua" {
        header = header.field(
            "Note",
            "Not described. Add {description='...'} to hl.bind in hyprland.lua",
        );
    }
    let header = header.build();

    let selection = FzfWrapper::builder()
        .prompt(format!("{} ", char::from(NerdFont::Wrench)))
        .header(header)
        .responsive_layout()
        .items(options)
        .padded()
        .select_one()?;

    match selection {
        crate::menu_utils::DialogOutcome::Submitted(item) => Ok(item.action),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(SubmenuAction::Back),
    }
}

pub(super) fn open_hyprland_config() -> Result<()> {
    open_in_editor(&hyprland_config_path())
}

pub(super) fn open_config() -> Result<()> {
    open_in_editor(&config_path())
}

/// Open `path` in the user's editor (nvim by default).
fn open_in_editor(path: &Path) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());

    FzfWrapper::message(&format!("Opening {} in {}…", path.display(), editor))?;

    let status = Command::new(&editor)
        .arg(path)
        .status()
        .with_context(|| format!("Failed to open editor '{editor}'"))?;

    if !status.success() {
        anyhow::bail!("Editor exited with non-zero status: {status:?}");
    }
    Ok(())
}

/// Translate the config-style action string back into an `instantwmctl
/// action` call. `describe()` renders actions as `name arg1 arg2 ...`, so the
/// first token is the action name and the rest are its args.
pub(super) fn run_binding(row: &KeybindRow) -> Result<()> {
    if row.action.starts_with("sequence [") {
        crate::menu_utils::FzfWrapper::message(
            "Sequences can't be triggered individually. Edit the config to change them.",
        )?;
        return Ok(());
    }

    let mut parts = row.action.splitn(2, ' ');
    let name = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("");

    let mut cmd: Vec<String> = vec!["action".to_string(), name.to_string()];
    if !args.is_empty() {
        cmd.extend(args.split_whitespace().map(str::to_string));
    }

    instantwmctl::run(&cmd)
        .map_err(|err| anyhow!("failed to run action '{}': {err}", row.action))?;
    Ok(())
}
