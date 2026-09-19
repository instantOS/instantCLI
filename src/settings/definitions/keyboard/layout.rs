//! Keyboard layout setting for desktop sessions

use anyhow::Result;
use std::collections::HashMap;

use crate::common::compositor::CompositorType;
use crate::common::xkb;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper, MenuKey, MenuKeybind};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::ui::catppuccin::{colors, format_icon, format_icon_colored};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::ui::{Level, emit};

use super::common::{
    KeyboardLayoutKeys, LayoutChoice, VariantChoice, apply_keyboard_layouts, current_gnome_layouts,
    current_instantwm_layouts, current_niri_layouts, current_sway_layout_names,
    current_x11_layouts, join_layout_codes, map_layout_names_to_codes, parse_xkb_layouts,
    parse_xkb_variants, split_layout_codes,
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
        variant: Option<String>,
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
                variant,
                name,
                position,
                total,
            } => {
                let mut builder = PreviewBuilder::new().header(NerdFont::Keyboard, name).line(
                    colors::TEAL,
                    Some(NerdFont::Tag),
                    &format!("Code: {}", code),
                );

                if let Some(variant) = variant {
                    builder = builder.line(
                        colors::TEAL,
                        Some(NerdFont::Gear),
                        &format!("Variant: {variant}"),
                    );
                }

                if *total > 1 {
                    builder = builder.line(
                        colors::TEAL,
                        Some(NerdFont::List),
                        &format!("Priority: {} of {}", position + 1, total),
                    );
                }

                builder = builder.blank().separator().blank();

                if *total > 1 {
                    builder =
                        builder.subtext("Ctrl+V to set variant; select for priority or actions");
                } else {
                    builder = builder.subtext("Ctrl+V to set variant; select to change or remove");
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
    SetVariant,
    Replace,
    Duplicate,
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
            LayoutActionItem::SetVariant => {
                format!("{} Set variant", format_icon(NerdFont::Gear))
            }
            LayoutActionItem::Replace => format!("{} Replace", format_icon(NerdFont::Sync)),
            LayoutActionItem::Duplicate => {
                format!("{} Duplicate", format_icon(NerdFont::ContentCopy))
            }
            LayoutActionItem::Remove => format!("{} Remove", format_icon(NerdFont::Minus)),
            LayoutActionItem::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let text = match self {
            LayoutActionItem::MoveUp => "Increase priority (will be tried first when switching)",
            LayoutActionItem::MoveDown => "Decrease priority",
            LayoutActionItem::SetVariant => {
                "Choose a keyboard variant for this layout (e.g. nodeadkeys)"
            }
            LayoutActionItem::Replace => "Replace this layout with a different one",
            LayoutActionItem::Duplicate => "Add a copy of this layout right below it",
            LayoutActionItem::Remove => "Remove this layout",
            LayoutActionItem::Back => "Return to layout list",
        };
        FzfPreview::Text(text.to_string())
    }
}

/// Actions bound to menu keys on the layout list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutListAction {
    /// Open the variant picker for the highlighted layout.
    SetVariant,
}

fn layout_list_keybinds() -> Result<Vec<MenuKeybind<LayoutListAction>>> {
    Ok(vec![MenuKeybind::new(
        MenuKey::new("ctrl-v")?,
        "set variant",
        LayoutListAction::SetVariant,
    )])
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
            let (base, variant) = xkb::split_layout_variant(code);
            let base_name = code_to_name
                .get(base)
                .cloned()
                .unwrap_or_else(|| base.to_string());
            let name = match variant {
                Some(variant) => format!("{base_name} ({variant})"),
                None => base_name,
            };
            LayoutMenuItem::Layout {
                code: code.clone(),
                variant: variant.map(str::to_string),
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
    actions.push(LayoutActionItem::SetVariant);
    actions.push(LayoutActionItem::Replace);
    actions.push(LayoutActionItem::Duplicate);
    if total > 1 {
        actions.push(LayoutActionItem::Remove);
    }
    actions.push(LayoutActionItem::Back);

    match FzfWrapper::menu().items(actions).padded().select_one()? {
        crate::menu_utils::DialogOutcome::Submitted(LayoutActionItem::MoveUp) => {
            active_codes.swap(position, position - 1);
            Ok(Some(true))
        }
        crate::menu_utils::DialogOutcome::Submitted(LayoutActionItem::MoveDown) => {
            active_codes.swap(position, position + 1);
            Ok(Some(true))
        }
        crate::menu_utils::DialogOutcome::Submitted(LayoutActionItem::SetVariant) => {
            if set_variant(ctx, active_codes, all_layouts, position, code)? {
                Ok(Some(true))
            } else {
                Ok(None)
            }
        }
        crate::menu_utils::DialogOutcome::Submitted(LayoutActionItem::Replace) => {
            if let Some(new_code) = select_layout(ctx, all_layouts, active_codes, Some(code))? {
                active_codes[position] = new_code;
                Ok(Some(true))
            } else {
                Ok(Some(false))
            }
        }
        crate::menu_utils::DialogOutcome::Submitted(LayoutActionItem::Duplicate) => {
            active_codes.insert(position + 1, code.to_string());
            ctx.emit_info("settings.keyboard.duplicated", "Layout duplicated");
            Ok(Some(true))
        }
        crate::menu_utils::DialogOutcome::Submitted(LayoutActionItem::Remove) => {
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
    if let Some(code) = select_layout(ctx, all_layouts, active_codes, None)? {
        active_codes.push(code);
        ctx.emit_info("settings.keyboard.added", "Layout added");
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Menu rows of the variant picker for one layout.
#[derive(Clone)]
enum VariantMenuItem {
    Clear,
    Choice(VariantChoice),
}

impl FzfSelectable for VariantMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            VariantMenuItem::Clear => format!(
                "{} No variant (default)",
                format_icon_colored(NerdFont::Minus, colors::OVERLAY0)
            ),
            VariantMenuItem::Choice(choice) => {
                format!("{} {}", format_icon(NerdFont::Gear), choice.name)
            }
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            VariantMenuItem::Clear => {
                FzfPreview::Text("Use the layout without a variant".to_string())
            }
            VariantMenuItem::Choice(choice) => choice.fzf_preview(),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            VariantMenuItem::Clear => "__no_variant__".to_string(),
            VariantMenuItem::Choice(choice) => choice.fzf_key(),
        }
    }
}

/// Pick a variant for a layout code via the variant picker.
///
/// Returns the updated code (`base(variant)` after choosing, bare `base`
/// after clearing), or `None` when the picker was cancelled, the layout has
/// no variants, or the selection would change nothing.
fn pick_variant(
    ctx: &mut SettingsContext,
    all_layouts: &[LayoutChoice],
    code: &str,
) -> Result<Option<String>> {
    let (base, current_variant) = xkb::split_layout_variant(code);
    let variants = match parse_xkb_variants() {
        Ok(variants) => variants,
        Err(e) => {
            ctx.emit_info(
                "settings.keyboard.variant_parse_error",
                &format!("Failed to parse keyboard variants: {e}"),
            );
            return Ok(None);
        }
    };
    let Some(choices) = variants.get(base) else {
        ctx.emit_info(
            "settings.keyboard.no_variants",
            &format!("No variants available for layout '{base}'"),
        );
        return Ok(None);
    };

    let base_name = all_layouts
        .iter()
        .find(|layout| layout.code == base)
        .map(|layout| layout.name.clone())
        .unwrap_or_else(|| base.to_string());

    let mut items = vec![VariantMenuItem::Clear];
    items.extend(choices.iter().map(|choice| {
        let mut choice = choice.clone();
        if current_variant.is_some_and(|current| current == choice.code) {
            choice.name.push_str(" (current)");
        }
        VariantMenuItem::Choice(choice)
    }));

    match FzfWrapper::menu()
        .header(format!("Variant for {base_name}"))
        .prompt("Variant")
        .items(items)
        .select_one()?
    {
        crate::menu_utils::DialogOutcome::Submitted(VariantMenuItem::Choice(choice)) => {
            let new_code = format!("{base}({})", choice.code);
            if new_code == code {
                return Ok(None);
            }
            ctx.emit_info(
                "settings.keyboard.variant_set",
                &format!("Variant set to {}", choice.code),
            );
            Ok(Some(new_code))
        }
        crate::menu_utils::DialogOutcome::Submitted(VariantMenuItem::Clear) => {
            if current_variant.is_none() {
                return Ok(None);
            }
            ctx.emit_info("settings.keyboard.variant_cleared", "Variant removed");
            Ok(Some(base.to_string()))
        }
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
    }
}

/// Apply the variant picker to the layout at `position`.
///
/// Returns `true` when the active codes changed and should be reapplied.
fn set_variant(
    ctx: &mut SettingsContext,
    active_codes: &mut [String],
    all_layouts: &[LayoutChoice],
    position: usize,
    code: &str,
) -> Result<bool> {
    match pick_variant(ctx, all_layouts, code)? {
        Some(new_code) => {
            active_codes[position] = new_code;
            Ok(true)
        }
        None => Ok(false),
    }
}

fn select_layout(
    ctx: &mut SettingsContext,
    all_layouts: &[LayoutChoice],
    active_codes: &[String],
    exclude_code: Option<&str>,
) -> Result<Option<String>> {
    let available: Vec<LayoutChoice> = if let Some(exclude) = exclude_code {
        let (exclude_base, _) = xkb::split_layout_variant(exclude);
        all_layouts
            .iter()
            .filter(|l| l.code != exclude_base)
            .cloned()
            .collect()
    } else {
        all_layouts.to_vec()
    };

    if available.is_empty() {
        return Ok(None);
    }

    let keybinds = layout_list_keybinds()?;
    loop {
        let result = FzfWrapper::builder()
            .header("Select Keyboard Layout")
            .prompt("Layout")
            .items(available.clone())
            .keybinds(&keybinds)
            .select()?;

        match result {
            crate::menu_utils::DialogOutcome::Submitted(selection) => {
                let selected = selection.items.into_iter().next();
                match selection.action {
                    Some(LayoutListAction::SetVariant) => {
                        if let Some(choice) = selected
                            && let Some(new_code) = pick_variant(ctx, all_layouts, &choice.code)?
                        {
                            if active_codes.iter().any(|c| c == &new_code) {
                                ctx.emit_info(
                                    "settings.keyboard.already_active",
                                    &format!("Layout '{new_code}' is already active"),
                                );
                                continue;
                            }
                            return Ok(Some(new_code));
                        }
                    }
                    None => match selected {
                        Some(choice) => {
                            if active_codes.iter().any(|c| c == &choice.code) {
                                if let Some(new_code) =
                                    pick_variant(ctx, all_layouts, &choice.code)?
                                {
                                    if active_codes.iter().any(|c| c == &new_code) {
                                        ctx.emit_info(
                                            "settings.keyboard.already_active",
                                            &format!("Layout '{new_code}' is already active"),
                                        );
                                        continue;
                                    }
                                    return Ok(Some(new_code));
                                }
                                continue;
                            }
                            return Ok(Some(choice.code));
                        }
                        None => return Ok(None),
                    },
                }
            }
            crate::menu_utils::DialogOutcome::Cancelled => return Ok(None),
        }
    }
}

impl Setting for KeyboardLayout {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.keyboard_layout")
            .title("Keyboard Layout")
            .icon(NerdFont::Keyboard)
            .summary("Select one or more keyboard layouts for the current desktop session (e.g., us, de, fr).\n\nHighlight a layout and press Ctrl+V to choose a variant (e.g. de nodeadkeys). Supports niri, Sway, GNOME, InstantWM, and X11 window managers. Use the TTY and login screen settings for system-wide layouts.")
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
            let (base, _) = xkb::split_layout_variant(code);
            all_layouts.iter().any(|layout| layout.code == base)
        });

        let code_to_name: HashMap<String, String> = all_layouts
            .iter()
            .map(|l| (l.code.clone(), l.name.clone()))
            .collect();

        let keybinds = layout_list_keybinds()?;
        let mut changed = false;

        loop {
            let items = build_layout_menu_items(&active_codes, &code_to_name);

            match FzfWrapper::menu()
                .items(items)
                .padded()
                .keybinds(&keybinds)
                .select()?
            {
                crate::menu_utils::DialogOutcome::Submitted(selection) => {
                    let selected = selection.items.into_iter().next();
                    match selection.action {
                        Some(LayoutListAction::SetVariant) => match selected {
                            Some(LayoutMenuItem::Layout { code, position, .. }) => {
                                if set_variant(
                                    ctx,
                                    &mut active_codes,
                                    &all_layouts,
                                    position,
                                    &code,
                                )? {
                                    changed = true;
                                }
                            }
                            _ => ctx.emit_info(
                                "settings.keyboard.variant_hint",
                                "Highlight a layout, then press Ctrl+V to set its variant",
                            ),
                        },
                        None => match selected {
                            Some(LayoutMenuItem::Layout { code, position, .. }) => {
                                if let Some(action) = handle_layout_action(
                                    ctx,
                                    &mut active_codes,
                                    &all_layouts,
                                    &code,
                                    position,
                                )? && action
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
                        },
                    }
                }
                crate::menu_utils::DialogOutcome::Cancelled => break,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_items_display_variants_behind_the_base_name() {
        let code_to_name = HashMap::from([("de".to_string(), "German".to_string())]);
        let active = vec!["us".to_string(), "de(nodeadkeys)".to_string()];

        let items = build_layout_menu_items(&active, &code_to_name);

        let LayoutMenuItem::Layout {
            code,
            variant,
            name,
            ..
        } = &items[1]
        else {
            panic!("expected a layout item");
        };
        assert_eq!(code, "de(nodeadkeys)");
        assert_eq!(variant.as_deref(), Some("nodeadkeys"));
        assert_eq!(name, "German (nodeadkeys)");

        // Unknown base codes fall back to the raw code, with or without
        // a variant.
        let LayoutMenuItem::Layout { name, .. } = &items[0] else {
            panic!("expected a layout item");
        };
        assert_eq!(name, "us");
    }
}
