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
    binding: String,
    action: String,
    mode: Option<String>,
    origin: String,
}

#[derive(Debug, Clone)]
struct KeybindRow {
    binding: String,
    action: String,
    mode: String,
    origin: String,
}

impl KeybindRow {
    fn from_json(j: KeybindRowJson) -> Self {
        Self {
            binding: j.binding,
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
            "config" => (hex_to_ansi_fg(colors::YELLOW), "your config"),
            _ => (hex_to_ansi_fg(colors::SUBTEXT0), "default"),
        };
        format!(
            "{binding_fg}{binding}{RESET}  {action_fg}{action}{RESET}  \
             {mode_fg}{mode}{RESET}  {origin_fg}{origin_label}{RESET}",
            binding = self.binding,
            action = self.action,
            mode = self.mode,
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Keyboard, "Keybinding")
            .field("Binding", &self.binding)
            .field("Action", &self.action)
            .field("Mode", &self.mode)
            .blank();
        if self.origin == "config" {
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
        anyhow!(
            "instantWM isn't running or `instantwmctl keybinds` failed: {err}\n\
             Start instantWM first, then run `ins keyhelp`."
        )
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
        row.binding
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

    let bind_w = rows.iter().map(|r| r.binding.len()).max().unwrap_or(0);
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
        let origin = if r.origin == "config" { "config" } else { "default" };
        println!(
            "{:<bw$} | {:<aw$} | {:5} | {:7}",
            r.binding, r.action, r.mode, origin,
            bw = bind_w,
            aw = act_w
        );
    }
    Ok(())
}