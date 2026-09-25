//! Shared keyboard utilities

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::common::compositor::{CompositorType, niri, sway};
use crate::common::instantwmctl;
use crate::common::xkb::{self, XkbVariant};
use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::settings::context::SettingsContext;
use crate::settings::store::StringSettingKey;
use crate::ui::catppuccin::{colors, format_icon};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;
use serde::Deserialize;
use serde_json::Value;
use which::which;

pub struct KeyboardLayoutKeys {
    pub sway: StringSettingKey,
    pub x11: StringSettingKey,
    pub gnome: StringSettingKey,
    pub instantwm: StringSettingKey,
    pub niri: StringSettingKey,
}

impl KeyboardLayoutKeys {
    pub fn new() -> Self {
        Self {
            sway: StringSettingKey::new("language.keyboard.sway", ""),
            x11: StringSettingKey::new("language.keyboard.x11", ""),
            gnome: StringSettingKey::new("language.keyboard.gnome", ""),
            instantwm: StringSettingKey::new("language.keyboard.instantwm", ""),
            niri: StringSettingKey::new("language.keyboard.niri", ""),
        }
    }
}

use std::sync::OnceLock;

static CACHED_LAYOUTS: OnceLock<Vec<LayoutChoice>> = OnceLock::new();
static CACHED_VARIANTS: OnceLock<BTreeMap<String, Vec<VariantChoice>>> = OnceLock::new();

#[derive(Clone)]
pub struct LayoutChoice {
    pub code: String,
    pub name: String,
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
            .blank()
            .separator()
            .blank()
            .subtext("Enter to select, Ctrl+V to choose variant")
            .build()
    }
}

pub fn parse_xkb_layouts() -> Result<Vec<LayoutChoice>> {
    if let Some(layouts) = CACHED_LAYOUTS.get() {
        return Ok(layouts.clone());
    }
    let layouts = load_xkb_layouts()?;
    let _ = CACHED_LAYOUTS.set(layouts.clone());
    Ok(layouts)
}

fn load_xkb_layouts() -> Result<Vec<LayoutChoice>> {
    let path = xkb::xkb_rules_path();
    let file = File::open(&path).with_context(|| format!("Failed to open {}", path.display()))?;
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
                layouts.push(LayoutChoice { code, name });
            }
        }
    }

    Ok(layouts)
}

/// One keyboard variant offered for a layout in the settings menu.
#[derive(Clone)]
pub struct VariantChoice {
    pub code: String,
    pub name: String,
}

impl FzfSelectable for VariantChoice {
    fn fzf_display_text(&self) -> String {
        format!("{} {}", format_icon(NerdFont::Gear), self.name)
    }

    fn fzf_key(&self) -> String {
        self.code.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        PreviewBuilder::new()
            .header(NerdFont::Gear, &self.name)
            .line(
                colors::TEAL,
                Some(NerdFont::Tag),
                &format!("Variant: {}", self.code),
            )
            .build()
    }
}

/// Parse the `! variant` section of `evdev.lst` into variants grouped by
/// layout code.
pub fn parse_xkb_variants() -> Result<&'static BTreeMap<String, Vec<VariantChoice>>> {
    if let Some(variants) = CACHED_VARIANTS.get() {
        return Ok(variants);
    }
    let variants = load_xkb_variants()?;
    let _ = CACHED_VARIANTS.set(variants);
    Ok(CACHED_VARIANTS.get().unwrap())
}

fn load_xkb_variants() -> Result<BTreeMap<String, Vec<VariantChoice>>> {
    let path = xkb::xkb_rules_path();
    let file = File::open(&path).with_context(|| format!("Failed to open {}", path.display()))?;
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .collect::<std::io::Result<_>>()
        .context("Failed to read XKB rules list")?;

    Ok(
        xkb::parse_xkb_variant_lines(lines.iter().map(String::as_str))
            .into_iter()
            .map(|(layout, variants)| {
                (
                    layout,
                    variants
                        .into_iter()
                        .map(|variant: XkbVariant| VariantChoice {
                            code: variant.code,
                            name: variant.name,
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

pub fn split_layout_codes(value: &str) -> Vec<String> {
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

pub fn join_layout_codes(codes: &[String]) -> String {
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

pub fn current_x11_layouts() -> Vec<String> {
    let output = match Command::new("setxkbmap").arg("-query").output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let mut layouts = Vec::new();
    let mut variants = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("layout:") {
            layouts = positional_csv(rest);
        } else if let Some(rest) = trimmed.strip_prefix("variant:") {
            variants = positional_csv(rest);
        }
    }

    xkb::merge_layout_variants(&layouts, &variants)
}

/// Split a positional XKB comma list, keeping empty segments so that the
/// positions of later entries survive (`,nodeadkeys` has two entries).
fn positional_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| part.trim().to_string())
        .collect()
}

/// Get current GNOME keyboard layouts from gsettings
pub fn current_gnome_layouts() -> Option<Vec<String>> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.input-sources", "sources"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_gnome_sources(&stdout)
}

/// Parse GNOME sources string into layout codes
fn parse_gnome_sources(sources_str: &str) -> Option<Vec<String>> {
    let trimmed = sources_str.trim();
    if trimmed == "@as []" {
        return Some(Vec::new());
    }

    let content = trimmed.strip_prefix('[')?.strip_suffix(']')?.trim();
    if content.is_empty() {
        return Some(Vec::new());
    }

    let mut layouts = Vec::new();
    for tuple in content.split("), (") {
        let clean = tuple
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim()
            .trim_matches('\'');

        let parts: Vec<&str> = clean.split("', '").collect();
        if parts.len() == 2 {
            let layout_code = parts[1].trim().trim_matches('\'');
            if !layout_code.is_empty() {
                layouts.push(normalize_gnome_layout_code(layout_code));
            }
        }
    }

    if layouts.is_empty() {
        None
    } else {
        Some(layouts)
    }
}

/// GNOME spells variants `layout+variant` (e.g. `de+nodeadkeys`); normalize
/// to the stored `layout(variant)` spelling.
fn normalize_gnome_layout_code(code: &str) -> String {
    match code.split_once('+') {
        Some((layout, variant)) if !layout.is_empty() && !variant.is_empty() => {
            format!("{layout}({variant})")
        }
        _ => code.to_string(),
    }
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

pub fn current_sway_layout_names() -> Option<Vec<String>> {
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

/// Layouts instantWM currently has configured, as `name(variant)` codes.
pub fn current_instantwm_layouts() -> Option<Vec<String>> {
    let layouts: Vec<InstantWmLayout> = instantwmctl::json(["keyboard", "list"]).ok()?;

    let codes: Vec<String> = layouts.iter().map(layout_code).collect();
    if codes.is_empty() { None } else { Some(codes) }
}

/// One entry of `instantwmctl --json keyboard list`.
#[derive(Debug, Clone, Deserialize)]
struct InstantWmLayout {
    name: String,
    #[serde(default)]
    variant: Option<String>,
}

/// Render an entry as the stored `name(variant)` spelling, so variants survive
/// the round-trip through `instantwmctl keyboard set`.
fn layout_code(layout: &InstantWmLayout) -> String {
    match layout.variant.as_deref() {
        Some(variant) if !variant.is_empty() => format!("{}({variant})", layout.name),
        _ => layout.name.clone(),
    }
}

pub fn current_niri_layouts() -> Option<Vec<String>> {
    niri::current_keyboard_layout_codes().ok()
}

pub fn map_layout_names_to_codes(names: &[String], layouts: &[LayoutChoice]) -> Vec<String> {
    let variants = parse_xkb_variants().ok();
    map_layout_names_from_sources(names, layouts, variants)
}

fn map_layout_names_from_sources(
    names: &[String],
    layouts: &[LayoutChoice],
    variants: Option<&BTreeMap<String, Vec<VariantChoice>>>,
) -> Vec<String> {
    let mut map = HashMap::new();
    let mut normalized_map = HashMap::new();
    for layout in layouts {
        map.insert(layout.name.clone(), layout.code.clone());
        normalized_map.insert(layout.name.to_lowercase(), layout.code.clone());
    }

    if let Some(variants) = variants {
        for (layout_code, var_choices) in variants {
            for var in var_choices {
                let full_code = format!("{layout_code}({})", var.code);
                map.insert(var.name.clone(), full_code.clone());
                normalized_map.insert(var.name.to_lowercase(), full_code);
            }
        }
    }

    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        let code = map
            .get(name)
            .cloned()
            .or_else(|| normalized_map.get(&name.to_lowercase()).cloned());
        if let Some(code) = code
            && seen.insert(code.clone())
        {
            result.push(code);
        }
    }

    result
}

pub fn list_keymaps() -> Result<Vec<String>> {
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

pub fn current_vconsole_keymap() -> Option<String> {
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

pub fn current_x11_layout() -> Option<String> {
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

pub fn ensure_localectl(ctx: &mut SettingsContext, code: &str, message: &str) -> bool {
    if which("localectl").is_err() {
        ctx.emit_unsupported(code, message);
        return false;
    }
    true
}

/// Apply keyboard layout(s) via swaymsg, niri msg, setxkbmap, instantwmctl, or gsettings depending on compositor
///
/// Codes use the `layout(variant)` spelling, e.g. `us,de(nodeadkeys)`.
pub fn apply_keyboard_layouts(codes: &[String], compositor: &CompositorType) -> Result<()> {
    if join_layout_codes(codes).is_empty() {
        bail!("No keyboard layouts selected");
    }
    let joined: String = codes
        .iter()
        .map(|code| xkb::base_layout(code))
        .collect::<Vec<_>>()
        .join(",");
    let variants = xkb::positional_variants(codes);

    match compositor {
        CompositorType::Sway => {
            let variant_arg = variants.as_deref().unwrap_or("");
            let cmd = format!(
                "input type:keyboard xkb_layout \"{joined}\" xkb_variant \"{variant_arg}\""
            );
            sway::swaymsg(&cmd)?;
        }
        CompositorType::Gnome => {
            apply_gnome_keyboard_layouts(codes)?;
        }
        CompositorType::Niri => {
            niri::set_keyboard_layouts(codes)
                .with_context(|| format!("Failed to update niri keyboard layouts to '{joined}'"))?;
        }
        CompositorType::InstantWM => {
            // instantwmctl parses `layout(variant)` codes natively.
            let mut args = vec!["keyboard".to_string(), "set".to_string()];
            args.extend(codes.iter().cloned());
            instantwmctl::run(args).with_context(|| {
                format!("Failed to execute instantwmctl keyboard set for layout '{joined}'")
            })?;
        }
        _ if compositor.is_x11() => {
            let mut command = Command::new("setxkbmap");
            command.arg("-layout").arg(&joined);
            if let Some(variants) = variants {
                command.arg("-variant").arg(variants);
            }
            if let Some(options) = current_x11_options() {
                command.arg("-option").arg(options);
            }
            command
                .status()
                .with_context(|| format!("Failed to execute setxkbmap for layout '{joined}'"))?;
        }
        _ => bail!("Unsupported compositor for keyboard layout configuration"),
    }
    Ok(())
}

/// Apply keyboard layouts to GNOME via gsettings
pub fn apply_gnome_keyboard_layouts(codes: &[String]) -> Result<()> {
    let sources: Vec<String> = codes
        .iter()
        .map(|code| {
            let (layout, variant) = xkb::split_layout_variant(code);
            match variant {
                Some(variant) => format!("('xkb', '{layout}+{variant}')"),
                None => format!("('xkb', '{layout}')"),
            }
        })
        .collect();
    let sources_str = format!("[{}]", sources.join(", "));

    std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.input-sources",
            "sources",
            &sources_str,
        ])
        .status()
        .with_context(|| format!("Failed to set GNOME keyboard layouts to: {sources_str}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_sources_normalize_plus_variants() {
        let sources = parse_gnome_sources("[('xkb', 'us'), ('xkb', 'de+nodeadkeys')]")
            .expect("sources parse");

        assert_eq!(
            sources,
            vec!["us".to_string(), "de(nodeadkeys)".to_string()]
        );
    }

    #[test]
    fn instantwm_layout_codes_restore_variant_parentheses() {
        fn layout(name: &str, variant: Option<&str>) -> InstantWmLayout {
            InstantWmLayout {
                name: name.to_string(),
                variant: variant.map(str::to_string),
            }
        }

        assert_eq!(layout_code(&layout("us", None)), "us");
        assert_eq!(
            layout_code(&layout("de", Some("nodeadkeys"))),
            "de(nodeadkeys)"
        );
        assert_eq!(layout_code(&layout("de", Some("intl"))), "de(intl)");
        // An empty variant is the same as none.
        assert_eq!(layout_code(&layout("de", Some(""))), "de");
    }

    #[test]
    fn instantwm_layout_codes_are_read_from_the_json_payload() {
        let layouts: Vec<InstantWmLayout> = serde_json::from_str(
            r#"[{"name":"us","variant":null,"is_active":true},{"name":"de","variant":"nodeadkeys","is_active":false}]"#,
        )
        .expect("keyboard list payload");

        let codes: Vec<String> = layouts.iter().map(layout_code).collect();
        assert_eq!(codes, vec!["us".to_string(), "de(nodeadkeys)".to_string()]);
    }

    #[test]
    fn map_layout_names_resolves_base_and_variant_names() {
        let layouts = vec![
            LayoutChoice {
                code: "us".to_string(),
                name: "English (US)".to_string(),
            },
            LayoutChoice {
                code: "de".to_string(),
                name: "German".to_string(),
            },
        ];
        let variants = BTreeMap::from([(
            "de".to_string(),
            vec![VariantChoice {
                code: "nodeadkeys".to_string(),
                name: "German (nodeadkeys)".to_string(),
            }],
        )]);

        let names = vec![
            "English (US)".to_string(),
            "German (nodeadkeys)".to_string(),
        ];
        let resolved = map_layout_names_from_sources(&names, &layouts, Some(&variants));
        assert_eq!(
            resolved,
            vec!["us".to_string(), "de(nodeadkeys)".to_string()]
        );
    }
}
