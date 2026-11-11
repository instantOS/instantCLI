use crate::common::instantwmctl;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

use super::GLOBAL_MODE;
use super::derive_keywords;
use super::model::{ActionDoc, KeybindRow, KeybindRowJson};
use super::paths::hyprland_config_path;
use super::ranking::sort_keybinds;

/// Fetch the live binding list. Fails loudly if instantWM isn't running -
/// keyhelp is about the *running* config, not the file on disk.
pub(super) fn fetch_keybinds() -> Result<Vec<KeybindRow>> {
    let rows: Vec<KeybindRowJson> = instantwmctl::json(["keybinds"]).map_err(|err| {
        let msg = err.to_string();
        if msg.contains("version mismatch") {
            anyhow!(
                "`instantwmctl` and instantWM are different builds. \
                 Restart instantWM, then run `ins keyhelp`."
            )
        } else if msg.contains("deserialize") {
            anyhow!(
                "instantWM doesn't support `instantwmctl keybinds`. \
                 It's an older build. Restart instantWM, then run `ins keyhelp`."
            )
        } else {
            anyhow!(
                "instantWM isn't running or `instantwmctl keybinds` failed: {msg}\n\
                 Start instantWM first, then run `ins keyhelp`."
            )
        }
    })?;
    let mut keybinds: Vec<KeybindRow> = rows.into_iter().map(KeybindRow::from_json).collect();
    sort_keybinds(&mut keybinds);
    Ok(keybinds)
}

#[derive(Debug, Clone, Deserialize)]
struct HyprlandBindJson {
    #[serde(default)]
    key: String,
    #[serde(default)]
    keycode: i32,
    modmask: u32,
    #[serde(default)]
    dispatcher: String,
    #[serde(default)]
    arg: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    has_description: bool,
    #[serde(default)]
    submap: String,
}

fn decode_hyprland_modmask(modmask: u32) -> String {
    // Hyprland modmask: 1=SHIFT, 4=CTRL, 8=ALT (Mod1), 64=SUPER (Mod4)
    // Keep order SUPER, CTRL, ALT, SHIFT to match `SUPER + CTRL + X` style in hyprland.lua
    let mut parts = Vec::new();
    if modmask & 64 != 0 {
        parts.push("SUPER");
    }
    if modmask & 4 != 0 {
        parts.push("CTRL");
    }
    if modmask & 8 != 0 {
        parts.push("ALT");
    }
    if modmask & 1 != 0 {
        parts.push("SHIFT");
    }
    if modmask & 2 != 0 {
        parts.push("CAPS");
    }
    parts.join("+")
}

pub(super) fn fetch_hyprland_keybinds() -> Result<Vec<KeybindRow>> {
    let output = Command::new("hyprctl")
        .args(["binds", "-j"])
        .output()
        .context("Failed to execute hyprctl binds")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("hyprctl binds failed: {}", stderr);
    }
    let binds: Vec<HyprlandBindJson> =
        serde_json::from_slice(&output.stdout).context("Failed to parse hyprctl binds JSON")?;

    let lua_path = hyprland_config_path().display().to_string();
    let mut rows = Vec::new();
    for b in binds {
        let modifiers = decode_hyprland_modmask(b.modmask);
        // hyprctl gives key as e.g. "RETURN", "mouse:272", "F12"
        let key = if !b.key.is_empty() {
            b.key
        } else if b.keycode != 0 {
            format!("code:{}", b.keycode)
        } else {
            String::new()
        };
        // Skip empty binds (should not happen)
        if key.is_empty() && modifiers.is_empty() {
            continue;
        }
        let mode = if b.submap.is_empty() {
            GLOBAL_MODE.to_string()
        } else {
            b.submap
        };

        let (action, origin) = if b.has_description && !b.description.is_empty() {
            (
                b.description.clone(),
                "hyprland.lua (described)".to_string(),
            )
        } else {
            // Undocumented: keep dispatcher/arg but annotate
            let fallback = if b.dispatcher == "__lua" {
                format!("Undocumented (__lua arg {}) see {}", b.arg, lua_path)
            } else if b.dispatcher.is_empty() {
                format!("Undocumented see {}", lua_path)
            } else if b.arg.is_empty() {
                format!("{} not described. See {}", b.dispatcher, lua_path)
            } else {
                format!("{} {} not described. See {}", b.dispatcher, b.arg, lua_path)
            };
            (fallback, "hyprland.lua".to_string())
        };

        let keywords = derive_keywords(&action, &mode);
        rows.push(KeybindRow {
            modifiers,
            key,
            action,
            mode,
            origin,
            keywords,
        });
    }
    sort_keybinds(&mut rows);
    Ok(rows)
}

#[derive(Debug, Clone, Deserialize)]
struct ActionDocJson {
    name: String,
    description: Option<String>,
    arg_example: Option<String>,
}

/// Fetch `instantwmctl --json action --list` and return an action-name →
/// description map. Best-effort: an empty map degrades the preview to "no
/// description".
pub fn fetch_action_docs() -> HashMap<String, ActionDoc> {
    match instantwmctl::json::<Vec<ActionDocJson>, _, _>(["action", "--list"]) {
        Ok(entries) => entries
            .into_iter()
            .filter_map(|entry| {
                entry.description.map(|description| {
                    (
                        entry.name,
                        ActionDoc {
                            description,
                            arg_example: entry.arg_example,
                        },
                    )
                })
            })
            .collect(),
        Err(_) => HashMap::new(),
    }
}
