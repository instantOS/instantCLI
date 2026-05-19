//! Keyboard layout setting for desktop sessions

use anyhow::Result;
use std::collections::{HashMap, HashSet};

use crate::common::compositor::CompositorType;
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, select_one_with_style};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::{colors, format_icon, format_icon_colored};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::ui::{Level, emit};

use super::common::{
    KeyboardLayoutKeys, LayoutChoice, apply_keyboard_layouts, current_gnome_layouts,
    current_instantwm_layouts, current_niri_layouts, current_sway_layout_names,
    current_x11_layouts, join_layout_codes, map_layout_names_to_codes, parse_xkb_layouts,
    split_layout_codes,
};

pub struct KeyboardLayout;

impl KeyboardLayout {
    fn keys() -> KeyboardLayoutKeys {
        KeyboardLayoutKeys::new()
    }
}

#[derive(Clone)]
enum LayoutMenuItem {
    Layout {
        code: String,
        name: String,
        position: usize,
        total: usize,
    },
    Add,
    Back,
}

impl FzfSelectable for LayoutMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            LayoutMenuItem::Layout {
                name,
                position,
                total,
                ..
            } => {
                let priority = if *total > 1 {
                    format!(" [{}]", position + 1)
                } else {
                    String::new()
                };
                format!("{} {}{}", format_icon(NerdFont::Keyboard), name, priority)
            }
            LayoutMenuItem::Add => {
                format!(
                    "{} Add layout",
                    format_icon_colored(NerdFont::Plus, colors::GREEN)
                )
            }
            LayoutMenuItem::Back => {
                format!(
                    "{} Back",
                    format_icon_colored(NerdFont::ArrowLeft, colors::OVERLAY0)
                )
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            LayoutMenuItem::Layout {
                code,
                name,
                position,
                total,
            } => {
                let mut builder = PreviewBuilder::new().header(NerdFont::Keyboard, name).line(
                    colors::TEAL,
                    Some(NerdFont::Tag),
                    &format!("Code: {}", code),
                );

                if *total > 1 {
                    builder = builder
                        .line(
                            colors::TEAL,
                            Some(NerdFont::List),
                            &format!("Priority: {} of {}", position + 1, total),
                        )
                        .blank()
                        .separator()
                        .blank()
                        .subtext("Select to change priority or remove");
                }

                builder.build()
            }
            LayoutMenuItem::Add => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Layout")
                .text("Add a new keyboard layout")
                .blank()
                .text("You can have multiple layouts")
                .text("and switch between them.")
                .build(),
            LayoutMenuItem::Back => FzfPreview::Text("Return to settings".to_string()),
        }
    }
}

#[derive(Clone)]
enum LayoutActionItem {
    MoveUp,
    MoveDown,
    Replace,
    Remove,
    Back,
}

impl FzfSelectable for LayoutActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            LayoutActionItem::MoveUp => format!(
                "{} Move up (higher priority)",
                format_icon(NerdFont::ArrowUp)
            ),
            LayoutActionItem::MoveDown => format!(
                "{} Move down (lower priority)",
                format_icon(NerdFont::ArrowDown)
            ),
            LayoutActionItem::Replace => format!("{} Replace", format_icon(NerdFont::Sync)),
            LayoutActionItem::Remove => format!("{} Remove", format_icon(NerdFont::Minus)),
            LayoutActionItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let text = match self {
            LayoutActionItem::MoveUp => "Increase priority (will be tried first when switching)",
            LayoutActionItem::MoveDown => "Decrease priority",
            LayoutActionItem::Replace => "Replace this layout with a different one",
            LayoutActionItem::Remove => "Remove this layout",
            LayoutActionItem::Back => "Return to layout list",
        };
        FzfPreview::Text(text.to_string())
    }
}

fn build_layout_menu_items(
    active_codes: &[String],
    code_to_name: &HashMap<String, String>,
) -> Vec<LayoutMenuItem> {
    let total = active_codes.len();
    let mut items: Vec<LayoutMenuItem> = active_codes
        .iter()
        .enumerate()
        .map(|(position, code)| {
            let name = code_to_name
                .get(code)
                .cloned()
                .unwrap_or_else(|| code.clone());
            LayoutMenuItem::Layout {
                code: code.clone(),
                name,
                position,
                total,
            }
        })
        .collect();

    items.push(LayoutMenuItem::Add);
    items.push(LayoutMenuItem::Back);
    items
}

fn handle_layout_action(
    ctx: &mut SettingsContext,
    active_codes: &mut Vec<String>,
    all_layouts: &[LayoutChoice],
    code: &str,
    position: usize,
) -> Result<Option<bool>> {
    let total = active_codes.len();

    let mut actions = Vec::new();
    if position > 0 {
        actions.push(LayoutActionItem::MoveUp);
    }
    if position < total.saturating_sub(1) {
        actions.push(LayoutActionItem::MoveDown);
    }
    actions.push(LayoutActionItem::Replace);
    if total > 1 {
        actions.push(LayoutActionItem::Remove);
    }
    actions.push(LayoutActionItem::Back);

    match select_one_with_style(actions)? {
        Some(LayoutActionItem::MoveUp) => {
            active_codes.swap(position, position - 1);
            Ok(Some(true))
        }
        Some(LayoutActionItem::MoveDown) => {
            active_codes.swap(position, position + 1);
            Ok(Some(true))
        }
        Some(LayoutActionItem::Replace) => {
            if let Some(new_code) = select_layout(all_layouts, active_codes, Some(code))? {
                active_codes[position] = new_code;
                Ok(Some(true))
            } else {
                Ok(Some(false))
            }
        }
        Some(LayoutActionItem::Remove) => {
            active_codes.remove(position);
            ctx.emit_info("settings.keyboard.removed", "Layout removed");
            Ok(Some(true))
        }
        _ => Ok(None),
    }
}

fn add_layout(
    ctx: &mut SettingsContext,
    active_codes: &mut Vec<String>,
    all_layouts: &[LayoutChoice],
) -> Result<bool> {
    if let Some(code) = select_layout(all_layouts, active_codes, None)? {
        active_codes.push(code);
        ctx.emit_info("settings.keyboard.added", "Layout added");
        Ok(true)
    } else {
        Ok(false)
    }
}

fn select_layout(
    all_layouts: &[LayoutChoice],
    active_codes: &[String],
    exclude_code: Option<&str>,
) -> Result<Option<String>> {
    let active_set: HashSet<&str> = active_codes.iter().map(|s| s.as_str()).collect();

    let available: Vec<LayoutChoice> = all_layouts
        .iter()
        .filter(|l| {
            let dominated = active_set.contains(l.code.as_str());
            let is_excluded = exclude_code.is_some_and(|ex| ex == l.code);
            !dominated || is_excluded
        })
        .cloned()
        .collect();

    if available.is_empty() {
        return Ok(None);
    }

    let result = FzfWrapper::builder()
        .header("Select Keyboard Layout")
        .prompt("Layout")
        .select(available)?;

    match result {
        FzfResult::Selected(layout) => Ok(Some(layout.code)),
        _ => Ok(None),
    }
}

impl Setting for KeyboardLayout {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.keyboard_layout")
            .title("Keyboard Layout")
            .icon(NerdFont::Keyboard)
            .summary("Select one or more keyboard layouts for the current desktop session (e.g., us, de, fr).\n\nSupports niri, Sway, GNOME, InstantWM, and X11 window managers. Use the TTY and login screen settings for system-wide layouts.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let compositor = CompositorType::detect();
        let is_sway = matches!(compositor, CompositorType::Sway);
        let is_gnome = matches!(compositor, CompositorType::Gnome);
        let is_niri = matches!(compositor, CompositorType::Niri);
        let is_instantwm = matches!(compositor, CompositorType::InstantWM);
        let is_x11 = compositor.is_x11();

        if !is_sway && !is_gnome && !is_niri && !is_x11 && !is_instantwm {
            ctx.emit_unsupported(
                "settings.keyboard.unsupported",
                "Keyboard layout configuration is currently only supported on niri, Sway, GNOME, InstantWM, and X11 window managers.",
            );
            return Ok(());
        }

        let all_layouts = match parse_xkb_layouts() {
            Ok(l) => l,
            Err(e) => {
                ctx.emit_info(
                    "settings.keyboard.parse_error",
                    &format!("Failed to parse keyboard layouts: {e}"),
                );
                return Ok(());
            }
        };

        let keys = Self::keys();
        let current_layout_key = if is_sway {
            keys.sway
        } else if is_gnome {
            keys.gnome
        } else if is_niri {
            keys.niri
        } else if is_instantwm {
            keys.instantwm
        } else {
            keys.x11
        };

        let stored_codes = split_layout_codes(&ctx.string(current_layout_key));
        let mut active_codes = if stored_codes.is_empty() {
            if is_sway {
                current_sway_layout_names()
                    .map(|names| map_layout_names_to_codes(&names, &all_layouts))
                    .unwrap_or_default()
            } else if is_gnome {
                current_gnome_layouts().unwrap_or_default()
            } else if is_niri {
                current_niri_layouts().unwrap_or_default()
            } else if is_instantwm {
                current_instantwm_layouts().unwrap_or_default()
            } else {
                current_x11_layouts()
            }
        } else {
            stored_codes
        };

        active_codes.retain(|code| {
            all_layouts
                .iter()
                .any(|layout| layout.code == code.as_str())
        });

        let code_to_name: HashMap<String, String> = all_layouts
            .iter()
            .map(|l| (l.code.clone(), l.name.clone()))
            .collect();

        let mut changed = false;

        loop {
            let items = build_layout_menu_items(&active_codes, &code_to_name);

            match select_one_with_style(items)? {
                Some(LayoutMenuItem::Layout { code, position, .. }) => {
                    if let Some(action) =
                        handle_layout_action(ctx, &mut active_codes, &all_layouts, &code, position)?
                        && action
                    {
                        changed = true;
                    }
                }
                Some(LayoutMenuItem::Add) => {
                    if add_layout(ctx, &mut active_codes, &all_layouts)? {
                        changed = true;
                    }
                }
                _ => break,
            }
        }

        if changed && !active_codes.is_empty() {
            if let Err(e) = apply_keyboard_layouts(&active_codes, &compositor) {
                ctx.emit_info(
                    "settings.keyboard.apply_error",
                    &format!("Failed to apply keyboard layout: {e}"),
                );
                return Ok(());
            }

            let joined = join_layout_codes(&active_codes);
            ctx.set_string(current_layout_key, &joined);
            ctx.notify("Keyboard Layout", &format!("Set to: {joined}"));
        }

        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::KeyboardLayout))
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let compositor = CompositorType::detect();
        let is_sway = matches!(compositor, CompositorType::Sway);
        let is_gnome = matches!(compositor, CompositorType::Gnome);
        let is_niri = matches!(compositor, CompositorType::Niri);
        let is_instantwm = matches!(compositor, CompositorType::InstantWM);
        let is_x11 = compositor.is_x11();

        if !is_sway && !is_gnome && !is_niri && !is_x11 && !is_instantwm {
            return None;
        }

        let keys = Self::keys();
        let key = if is_sway {
            keys.sway
        } else if is_gnome {
            keys.gnome
        } else if is_niri {
            keys.niri
        } else if is_instantwm {
            keys.instantwm
        } else {
            keys.x11
        };
        let codes = split_layout_codes(&ctx.string(key));
        if codes.is_empty() {
            return None;
        }

        if let Err(e) = apply_keyboard_layouts(&codes, &compositor) {
            emit(
                Level::Warn,
                "settings.keyboard.restore_failed",
                &format!("Failed to restore keyboard layout: {e}"),
                None,
            );
        } else {
            emit(
                Level::Debug,
                "settings.keyboard.restored",
                &format!("Restored keyboard layout: {}", join_layout_codes(&codes)),
                None,
            );
        }

        Some(Ok(()))
    }
}
