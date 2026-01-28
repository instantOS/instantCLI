//! Keyboard layout setting

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::Command;

use crate::arch::annotations::{KeymapAnnotationProvider, annotate_list};
use crate::common::compositor::{CompositorType, sway};
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, select_one_with_style};
use crate::preview::{PreviewId, preview_command};
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::StringSettingKey;
use crate::ui::catppuccin::{colors, format_icon, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;
use serde_json::Value;
use which::which;

pub struct KeyboardLayout;
pub struct TtyKeymap;
pub struct LoginScreenLayout;

impl KeyboardLayout {
    const KEY_SWAY: StringSettingKey = StringSettingKey::new("language.keyboard.sway", "");
    const KEY_X11: StringSettingKey = StringSettingKey::new("language.keyboard.x11", "");
}

impl TtyKeymap {
    const KEY: StringSettingKey = StringSettingKey::new("language.keyboard.tty", "");
}

impl LoginScreenLayout {
    const KEY: StringSettingKey = StringSettingKey::new("language.keyboard.login", "");
}

#[derive(Clone)]
struct LayoutChoice {
    code: String,
    name: String,
    checked: bool,
}

impl FzfSelectable for LayoutChoice {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", format_icon(NerdFont::Keyboard), self.name)
    }

    fn fzf_key(&self) -> String {
        self.code.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Keyboard, &self.name)
            .line(
                colors::TEAL,
                Some(NerdFont::Tag),
                &format!("Code: {}", self.code),
            )
            .build()
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

fn parse_xkb_layouts() -> Result<Vec<LayoutChoice>> {
    let file = File::open("/usr/share/X11/xkb/rules/evdev.lst")
        .context("Failed to open /usr/share/X11/xkb/rules/evdev.lst")?;
    let reader = BufReader::new(file);

    let mut layouts = Vec::new();
    let mut in_layout_section = false;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed == "! layout" {
            in_layout_section = true;
            continue;
        }

        if trimmed == "! variant" {
            break;
        }

        if in_layout_section && !trimmed.starts_with('!') && !trimmed.is_empty() {
            let parts: Vec<&str> = trimmed.splitn(2, |c: char| c.is_whitespace()).collect();
            if parts.len() == 2 {
                let code = parts[0].trim().to_string();
                let name = parts[1].trim().to_string();
                layouts.push(LayoutChoice {
                    code,
                    name,
                    checked: false,
                });
            }
        }
    }

    Ok(layouts)
}

fn split_layout_codes(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for part in value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            result.push(trimmed.to_string());
        }
    }

    result
}

fn join_layout_codes(codes: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut cleaned = Vec::new();

    for code in codes {
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            cleaned.push(trimmed.to_string());
        }
    }

    cleaned.join(",")
}

pub(crate) fn current_x11_layouts() -> Vec<String> {
    let output = match Command::new("setxkbmap").arg("-query").output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("layout:") {
            return split_layout_codes(rest);
        }
    }

    Vec::new()
}

fn current_x11_options() -> Option<String> {
    let output = Command::new("setxkbmap").arg("-query").output().ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("options:") {
            let options = rest.trim();
            if options.is_empty() {
                return None;
            }
            return Some(options.to_string());
        }
    }

    None
}

fn current_sway_layout_names() -> Option<Vec<String>> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_inputs", "-r"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let data: Value = serde_json::from_slice(&output.stdout).ok()?;
    let inputs = data.as_array()?;

    let mut names = Vec::new();
    let mut seen = HashSet::new();

    for input in inputs {
        if input.get("type").and_then(|v| v.as_str()) != Some("keyboard") {
            continue;
        }

        if let Some(layouts) = input.get("xkb_layout_names").and_then(|v| v.as_array()) {
            for layout in layouts {
                if let Some(name) = layout.as_str()
                    && seen.insert(name.to_string())
                {
                    names.push(name.to_string());
                }
            }
        }
    }

    if names.is_empty() { None } else { Some(names) }
}

fn map_layout_names_to_codes(names: &[String], layouts: &[LayoutChoice]) -> Vec<String> {
    let mut map = HashMap::new();
    for layout in layouts {
        map.insert(layout.name.clone(), layout.code.clone());
    }

    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        if let Some(code) = map.get(name)
            && seen.insert(code.clone())
        {
            result.push(code.clone());
        }
    }

    result
}

fn list_keymaps() -> Result<Vec<String>> {
    let output = Command::new("localectl")
        .arg("list-keymaps")
        .output()
        .context("running localectl list-keymaps")?;

    if !output.status.success() {
        bail!(
            "localectl list-keymaps exited with status {:?}",
            output.status.code()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

pub(crate) fn current_vconsole_keymap() -> Option<String> {
    std::fs::read_to_string("/etc/vconsole.conf")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.trim_start().starts_with("KEYMAP="))
                .map(|line| {
                    line.trim_start()
                        .trim_start_matches("KEYMAP=")
                        .trim()
                        .to_string()
                })
        })
}

pub(crate) fn current_x11_layout() -> Option<String> {
    let output = Command::new("localectl").arg("status").output().ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("X11 Layout:") {
            let layout = rest.trim();
            if layout.is_empty() {
                return None;
            }
            let first = layout.split(',').next().unwrap_or(layout).trim();
            if first.is_empty() {
                return None;
            }
            return Some(first.to_string());
        }
    }

    None
}

fn ensure_localectl(ctx: &mut SettingsContext, code: &str, message: &str) -> bool {
    if which("localectl").is_err() {
        ctx.emit_unsupported(code, message);
        return false;
    }
    true
}

/// Apply keyboard layout(s) via swaymsg or setxkbmap depending on compositor
fn apply_keyboard_layouts(codes: &[String], compositor: &CompositorType) -> Result<()> {
    let joined = join_layout_codes(codes);
    if joined.is_empty() {
        bail!("No keyboard layouts selected");
    }

    if matches!(compositor, CompositorType::Sway) {
        let cmd = format!("input type:keyboard xkb_layout \"{joined}\"");
        sway::swaymsg(&cmd)?;
    } else if compositor.is_x11() {
        let mut command = std::process::Command::new("setxkbmap");
        command.arg("-layout").arg(&joined);
        if let Some(options) = current_x11_options() {
            command.arg("-option").arg(options);
        }
        command
            .status()
            .with_context(|| format!("Failed to execute setxkbmap for layout '{joined}'"))?;
    }
    Ok(())
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
            .summary("Select one or more keyboard layouts for the current desktop session (e.g., us, de, fr).\n\nSupports Sway and X11 window managers. Use the TTY and login screen settings for system-wide layouts.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let compositor = CompositorType::detect();
        let is_sway = matches!(compositor, CompositorType::Sway);
        let is_x11 = compositor.is_x11();

        if !is_sway && !is_x11 {
            ctx.emit_unsupported(
                "settings.keyboard.unsupported",
                "Keyboard layout configuration is currently only supported on Sway and X11 window managers.",
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

        let current_layout_key = if is_sway {
            Self::KEY_SWAY
        } else {
            Self::KEY_X11
        };

        let stored_codes = split_layout_codes(&ctx.string(current_layout_key));
        let mut active_codes = if stored_codes.is_empty() {
            if is_sway {
                current_sway_layout_names()
                    .map(|names| map_layout_names_to_codes(&names, &all_layouts))
                    .unwrap_or_default()
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
        let is_x11 = compositor.is_x11();

        if !is_sway && !is_x11 {
            return None;
        }

        let key = if is_sway {
            Self::KEY_SWAY
        } else {
            Self::KEY_X11
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

impl Setting for TtyKeymap {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.keyboard_tty")
            .title("TTY Keymap")
            .icon(NerdFont::Keyboard)
            .summary("Set the console (TTY) keymap via localectl set-keymap.\n\nAffects virtual consoles and login TTYs.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        if !ensure_localectl(
            ctx,
            "settings.keyboard.tty.no_systemd",
            "TTY keymap configuration requires systemd (localectl not found).",
        ) {
            return Ok(());
        }

        let keymaps = match list_keymaps() {
            Ok(list) => list,
            Err(err) => {
                ctx.emit_info(
                    "settings.keyboard.tty.list_failed",
                    &format!("Failed to list keymaps: {err}"),
                );
                return Ok(());
            }
        };

        if keymaps.is_empty() {
            ctx.emit_info(
                "settings.keyboard.tty.none",
                "No console keymaps reported by localectl.",
            );
            return Ok(());
        }

        let provider = KeymapAnnotationProvider;
        let choices = annotate_list(Some(&provider), keymaps);

        let current = if ctx.contains(Self::KEY.key) {
            ctx.string(Self::KEY)
        } else {
            current_vconsole_keymap().unwrap_or_default()
        };

        let initial_index = choices
            .iter()
            .position(|choice| choice.value == current)
            .unwrap_or(0);

        let result = FzfWrapper::builder()
            .header("Select TTY Keymap")
            .prompt("Keymap")
            .initial_index(initial_index)
            .select(choices)?;

        match result {
            FzfResult::Selected(choice) => {
                if let Err(err) =
                    ctx.run_command_as_root("localectl", ["set-keymap", choice.value.as_str()])
                {
                    ctx.emit_info(
                        "settings.keyboard.tty.apply_failed",
                        &format!("Failed to apply TTY keymap: {err}"),
                    );
                    return Ok(());
                }

                ctx.set_string(Self::KEY, &choice.value);
                ctx.notify("TTY Keymap", &format!("Set to: {}", choice.value));
            }
            FzfResult::Error(err) => bail!("fzf error: {err}"),
            _ => {}
        }

        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::TtyKeymap))
    }
}

impl Setting for LoginScreenLayout {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("language.keyboard_login")
            .title("Login Screen Layout")
            .icon(NerdFont::Keyboard)
            .summary("Set the keyboard layout for GDM/LightDM login screens via localectl set-x11-keymap.\n\nAffects the display manager and default X11 layout.")
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Action
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        if !ensure_localectl(
            ctx,
            "settings.keyboard.login.no_systemd",
            "Login screen layout configuration requires systemd (localectl not found).",
        ) {
            return Ok(());
        }

        let layouts = match parse_xkb_layouts() {
            Ok(list) => list,
            Err(err) => {
                ctx.emit_info(
                    "settings.keyboard.login.parse_failed",
                    &format!("Failed to parse XKB layouts: {err}"),
                );
                return Ok(());
            }
        };

        if layouts.is_empty() {
            ctx.emit_info(
                "settings.keyboard.login.none",
                "No XKB layouts found. Ensure xkeyboard-config is installed.",
            );
            return Ok(());
        }

        let current = if ctx.contains(Self::KEY.key) {
            ctx.string(Self::KEY)
        } else {
            current_x11_layout().unwrap_or_default()
        };

        let initial_index = layouts
            .iter()
            .position(|layout| layout.code == current)
            .unwrap_or(0);

        let result = FzfWrapper::builder()
            .header("Select Login Screen Layout")
            .prompt("Layout")
            .initial_index(initial_index)
            .select(layouts)?;

        match result {
            FzfResult::Selected(layout) => {
                if let Err(err) =
                    ctx.run_command_as_root("localectl", ["set-x11-keymap", layout.code.as_str()])
                {
                    ctx.emit_info(
                        "settings.keyboard.login.apply_failed",
                        &format!("Failed to apply login screen layout: {err}"),
                    );
                    return Ok(());
                }

                ctx.set_string(Self::KEY, &layout.code);
                ctx.notify(
                    "Login Screen Layout",
                    &format!("Set to: {} ({})", layout.name, layout.code),
                );
            }
            FzfResult::Error(err) => bail!("fzf error: {err}"),
            _ => {}
        }

        Ok(())
    }

    fn preview_command(&self) -> Option<String> {
        Some(preview_command(PreviewId::LoginScreenLayout))
    }
}
