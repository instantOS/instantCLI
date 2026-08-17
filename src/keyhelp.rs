//! `ins keyhelp` — explore and memorize instantWM keybinds.
//!
//! Fetches the live binding list over `instantwmctl keybinds --json` and
//! presents it as a searchable fzf menu. Selecting an entry shows its origin
//! and lets the user either run the action or open the config file in their
//! editor.

use crate::common::compositor::CompositorType;
use crate::common::instantwmctl;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Deserialize)]
struct KeybindRowJson {
    modifiers: String,
    key: String,
    action: String,
    mode: Option<String>,
    origin: String,
}

#[derive(Debug, Clone)]
struct KeybindRow {
    modifiers: String,
    key: String,
    action: String,
    mode: String,
    origin: String,
}

impl KeybindRow {
    /// Render the chord as the user would type it: empty modifiers → just the
    /// key, otherwise `Modifiers + Key`. Kept as a method (not a stored field)
    /// so the composition can't drift from the renderer — both call this.
    fn binding(&self) -> String {
        if self.modifiers.is_empty() {
            self.key.clone()
        } else {
            format!("{} + {}", self.modifiers, self.key)
        }
    }

    fn from_json(j: KeybindRowJson) -> Self {
        Self {
            modifiers: j.modifiers,
            key: j.key,
            action: j.action,
            mode: j.mode.unwrap_or_else(|| "global".to_string()),
            origin: j.origin,
        }
    }
}

impl FzfSelectable for KeybindRow {
    fn fzf_display_text(&self) -> String {
        let binding_fg = hex_to_ansi_fg(colors::GREEN);
        let action_fg = hex_to_ansi_fg(colors::TEXT);
        let mode_fg = hex_to_ansi_fg(colors::MAUVE);
        let (origin_fg, origin_label) = match self.origin.as_str() {
            "user" => (hex_to_ansi_fg(colors::YELLOW), "your config"),
            _ => (hex_to_ansi_fg(colors::SUBTEXT0), "default"),
        };
        let binding = self.binding();
        format!(
            "{binding_fg}{binding}{RESET}  {action_fg}{action}{RESET}  \
             {mode_fg}{mode}{RESET}  {origin_fg}{origin_label}{RESET}",
            action = self.action,
            mode = self.mode,
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Keyboard, "Keybinding")
            .field("Binding", &self.binding())
            .field("Action", &self.action)
            .field("Mode", &self.mode)
            .blank();
        if self.origin == "user" {
            builder = builder.text("Defined in your ~/.config/instantwm/config.toml.");
        } else {
            builder = builder.text("Built-in default — ships with instantWM.");
        }
        crate::menu_utils::FzfPreview::Text(builder.build_string())
    }
}

#[derive(Debug, Clone)]
struct ActionOption {
    label: String,
    key: String,
}

impl FzfSelectable for ActionOption {
    fn fzf_display_text(&self) -> String {
        let fg = hex_to_ansi_fg(colors::TEXT);
        format!("{fg}{label}{RESET}", label = self.label)
    }

    fn fzf_key(&self) -> String {
        self.key.clone()
    }
}

/// Entry point for `ins keyhelp`.
pub fn run_keyhelp() -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return print_text_list();
    }

    let compositor = CompositorType::detect();
    if !matches!(compositor, CompositorType::InstantWM) {
        eprintln!(
            "ins keyhelp: keybind listing is only supported on instantWM \
             (detected: {}).",
            compositor.name()
        );
        return Ok(());
    }

    let rows = fetch_keybinds()?;
    if rows.is_empty() {
        println!("No keybindings found — is instantWM running?");
        return Ok(());
    }

    let header = crate::menu_utils::HeaderBuilder::new(
        NerdFont::Keyboard,
        format!("instantWM keybinds  ·  {} bindings", rows.len()),
    )
    .subtitle("Select a binding to run it, or edit its config file")
    .build();

    let result = FzfWrapper::builder()
        .prompt(format!("{} ", char::from(NerdFont::Search)))
        .header(header)
        .responsive_layout()
        .select(rows)?;

    match result {
        FzfResult::Selected(row) => handle_select(row),
        FzfResult::Cancelled | FzfResult::MultiSelected(_) => Ok(()),
        FzfResult::Error(err) => Err(anyhow!(err)),
    }
}

/// Fetch the live binding list. Fails loudly if instantWM isn't running —
/// keyhelp is about the *running* config, not the file on disk.
fn fetch_keybinds() -> Result<Vec<KeybindRow>> {
    let rows: Vec<KeybindRowJson> = instantwmctl::json(["keybinds"]).map_err(|err| {
        let msg = err.to_string();
        if msg.contains("version mismatch") {
            anyhow!(
                "`instantwmctl` and instantWM are different builds — \
                 restart instantWM, then run `ins keyhelp`."
            )
        } else if msg.contains("deserialize") {
            anyhow!(
                "instantWM doesn't support `instantwmctl keybinds` — \
                 it's an older build. Restart instantWM, then run `ins keyhelp`."
            )
        } else {
            anyhow!(
                "instantWM isn't running or `instantwmctl keybinds` failed: {msg}\n\
                 Start instantWM first, then run `ins keyhelp`."
            )
        }
    })?;
    Ok(rows.into_iter().map(KeybindRow::from_json).collect())
}

/// What to do with a selected binding.
fn handle_select(row: KeybindRow) -> Result<()> {
    let options = vec![
        ActionOption {
            label: format!("Run: {}", row.action),
            key: "run".to_string(),
        },
        ActionOption {
            label: "Edit config.toml".to_string(),
            key: "edit".to_string(),
        },
    ];

    let header = crate::menu_utils::Header::default(format!(
        "{}  {}",
        char::from(NerdFont::Keyboard),
        row.binding()
    ).as_str());

    let result = FzfWrapper::builder()
        .prompt(format!("{} ", char::from(NerdFont::Wrench)))
        .header(header)
        .select(options)?;

    match result {
        FzfResult::Selected(o) if o.key == "run" => run_binding(&row),
        FzfResult::Selected(_) => open_config(),
        _ => Ok(()),
    }
}

/// Translate the config-style action string back into an `instantwmctl
/// action` call. `describe()` renders actions as `name arg1 arg2 ...`, so the
/// first token is the action name and the rest are its args.
fn run_binding(row: &KeybindRow) -> Result<()> {
    if row.action.starts_with("sequence [") {
        crate::menu_utils::FzfWrapper::message(
            "Sequences can't be triggered individually — edit the config to change them.",
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

/// Open the instantWM config file in the user's editor (nvim by default).
fn open_config() -> Result<()> {
    let path = config_path();
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());

    FzfWrapper::message(&format!(
        "Opening {} in {}…",
        path.display(),
        editor
    ))?;

    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| format!("Failed to open editor '{editor}'"))?;

    if !status.success() {
        anyhow::bail!("Editor exited with non-zero status: {status:?}");
    }
    Ok(())
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join("instantwm")
        .join("config.toml")
}

/// Non-tty fallback: print a plain-text table, like `ins assist list`.
fn print_text_list() -> Result<()> {
    let compositor = CompositorType::detect();
    if !matches!(compositor, CompositorType::InstantWM) {
        eprintln!(
            "ins keyhelp: keybind listing is only supported on instantWM \
             (detected: {}).",
            compositor.name()
        );
        return Ok(());
    }

    let rows = fetch_keybinds()?;
    if rows.is_empty() {
        println!("No keybindings found — is instantWM running?");
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
        let origin = if r.origin == "user" { "config" } else { "default" };
        println!(
            "{:<bw$} | {:<aw$} | {:5} | {:7}",
            r.binding(), r.action, r.mode, origin,
            bw = bind_w,
            aw = act_w
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows captured from `instantwmctl --json keybinds` on a real instantWM
    /// session. Covers every origin value, all four modes, and a mix of
    /// simple actions, args, spawns, and sequences.
    const SAMPLE_JSON: &str = include_str!("../tests/fixtures/keybinds.json");

    #[test]
    fn parses_sample_keybinds() {
        let rows: Vec<KeybindRowJson> =
            serde_json::from_str(SAMPLE_JSON).expect("fixture must be valid JSON");
        assert!(rows.len() >= 10, "fixture has {} rows", rows.len());

        // Origin enum names are serialized verbatim.
        let user_rows: Vec<_> = rows.iter().filter(|r| r.origin == "user").collect();
        assert_eq!(user_rows.len(), 3, "expected 3 user-defined bindings");
        assert!(user_rows.iter().all(|r| r.modifiers.contains("Super")));

        // Modes are preserved.
        let modes: std::collections::BTreeSet<String> =
            rows.iter().filter_map(|r| r.mode.clone()).collect();
        assert!(modes.contains("desktop"));
        assert!(modes.contains("placement"));
        assert!(modes.contains("prefix"));
        assert!(modes.contains("overview"));
    }

    fn row(modifiers: &str, key: &str, action: &str, mode: Option<&str>, origin: &str) -> KeybindRow {
        KeybindRow {
            modifiers: modifiers.to_string(),
            key: key.to_string(),
            action: action.to_string(),
            mode: mode.unwrap_or("global").to_string(),
            origin: origin.to_string(),
        }
    }

    #[test]
    fn binding_composes_modifiers_and_key() {
        assert_eq!(
            row("Super + Shift", "s", "x", None, "compiled_default").binding(),
            "Super + Shift + s"
        );
        assert_eq!(
            row("Super", "Return", "x", None, "user").binding(),
            "Super + Return"
        );
        // Bare keys (desktop/placement bindings) render without a modifier.
        assert_eq!(
            row("", "Return", "x", Some("desktop"), "compiled_default").binding(),
            "Return"
        );
        assert_eq!(
            row("", "BrightnessUp", "x", None, "compiled_default").binding(),
            "BrightnessUp"
        );
    }

    #[test]
    fn user_origin_renders_as_your_config() {
        let row = row("Super", "Return", "spawn kitty --single-instance", None, "user");
        let display = row.fzf_display_text();
        assert!(display.contains("your config"), "got: {display}");
        assert!(!display.contains("default"));
        assert!(display.contains("Super + Return"));
    }

    #[test]
    fn compiled_default_renders_as_default() {
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        let display = row.fzf_display_text();
        assert!(display.contains("default"));
        assert!(!display.contains("your config"));
    }

    #[test]
    fn run_binding_splits_action_name_and_args() {
        let row = row("Super", "i", "inc_master_count 1", None, "compiled_default");
        // Verify the split logic that `run_binding` relies on.
        let mut parts = row.action.splitn(2, ' ');
        assert_eq!(parts.next(), Some("inc_master_count"));
        assert_eq!(parts.next(), Some("1"));
    }

    #[test]
    fn sequence_actions_are_detected() {
        let row = row(
            "Super + Ctrl",
            "Shift",
            "sequence [set_mode default, spawn ins assist run sf]",
            None,
            "compiled_default",
        );
        assert!(row.action.starts_with("sequence ["));
    }
}