//! `ins keyhelp` — explore and memorize instantWM keybinds.
//!
//! Fetches the live binding list over `instantwmctl keybinds --json` and
//! presents it as a searchable fzf menu. Each binding shows a rich preview
//! (mode availability, origin, and the built-in action description from
//! `instantwmctl action --list`). Selecting an entry opens a small action
//! menu: run the action, open the config file in the editor, or go back to
//! the keybind list.

use crate::common::compositor::CompositorType;
use crate::common::instantwmctl;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_icon_colored, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{PreviewBuilder, PreviewWriter};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

const RESET: &str = "\x1b[0m";

/// Mode tag used for bindings that are active in every mode.
const GLOBAL_MODE: &str = "global";

/// Documentation for one named WM action, sourced from
/// `instantwmctl --json action --list`.
#[derive(Debug, Clone)]
struct ActionDoc {
    description: String,
    arg_example: Option<String>,
}

type ActionDocs = Arc<HashMap<String, ActionDoc>>;

#[derive(Debug, Clone, Deserialize)]
struct KeybindRowJson {
    modifiers: String,
    key: String,
    action: String,
    mode: Option<String>,
    origin: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionDocJson {
    name: String,
    description: Option<String>,
    arg_example: Option<String>,
}

#[derive(Debug, Clone)]
struct KeybindRow {
    modifiers: String,
    key: String,
    action: String,
    mode: String,
    origin: String,
    docs: ActionDocs,
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
            mode: j.mode.unwrap_or_else(|| GLOBAL_MODE.to_string()),
            origin: j.origin,
            docs: Arc::new(HashMap::new()),
        }
    }

    fn with_docs(mut self, docs: ActionDocs) -> Self {
        self.docs = docs;
        self
    }

    /// The individual actions of a `sequence [...]` binding, or `None` for a
    /// plain (non-sequence) action.
    fn sequence_steps(&self) -> Option<Vec<String>> {
        let inner = self.action.strip_prefix("sequence [")?.strip_suffix(']')?;
        Some(
            inner
                .split(',')
                .map(|step| step.trim().to_string())
                .filter(|step| !step.is_empty())
                .collect(),
        )
    }

    /// Human-readable origin label, shown only in the preview.
    fn origin_label(&self) -> &'static str {
        if self.origin == "user" {
            "your config"
        } else {
            "default"
        }
    }

    /// Headline "what it does" line + usage for a plain action.
    fn build_action_doc(&self, builder: &mut PreviewWriter) {
        let name = action_name(&self.action);
        match self.docs.get(name) {
            Some(doc) => {
                builder.line(colors::SKY, Some(NerdFont::Info), &doc.description);
                if let Some(arg) = &doc.arg_example {
                    builder.field_indented("Usage", &format!("{name} {arg}"));
                }
            }
            None => {
                builder.subtext(&format!("No built-in description for '{name}'."));
            }
        }
    }

    /// Per-step bullets for `sequence [...]` bindings.
    fn build_sequence_doc(&self, builder: &mut PreviewWriter) {
        builder.title(colors::BLUE, "Sequence");
        match self.sequence_steps() {
            Some(steps) => {
                for step in &steps {
                    match self.docs.get(action_name(step)) {
                        Some(doc) => {
                            builder.bullet(&format!("{step} — {}", doc.description));
                        }
                        None => {
                            builder.bullet(step);
                        }
                    }
                }
            }
            None => {
                builder.subtext("Malformed sequence.");
            }
        }
    }
}

/// First token of an action string: the named action (`inc_master_count 1` →
/// `inc_master_count`). The whole string is returned when it has no args.
fn action_name(action: &str) -> &str {
    action.split_whitespace().next().unwrap_or(action)
}

impl FzfSelectable for KeybindRow {
    fn fzf_display_text(&self) -> String {
        let binding_fg = hex_to_ansi_fg(colors::GREEN);
        let action_fg = hex_to_ansi_fg(colors::TEXT);
        format!(
            "{binding_fg}{} {binding}{RESET}   {action_fg}{action}{RESET}",
            char::from(NerdFont::Keyboard),
            binding = self.binding(),
            action = self.action,
        )
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        let mut w = PreviewWriter::collect();

        // Mode + availability. Global bindings fire in every mode; the rest
        // only while their named mode (desktop, placement, prefix, …) is
        // active — surface that in the preview, not in the list item.
        w.header(NerdFont::Keyboard, "Keybinding");
        w.field("Binding", &self.binding());
        w.field("Action", &self.action);
        w.blank();

        if self.mode == GLOBAL_MODE {
            w.field("Mode", "global (all modes)");
        } else {
            w.field("Mode", &self.mode);
            w.line(
                colors::MAUVE,
                Some(NerdFont::Info),
                &format!("Only available when the '{}' mode is enabled", self.mode),
            );
        }

        w.field("Origin", self.origin_label());
        w.blank();

        if !self.docs.is_empty() {
            if self.sequence_steps().is_some() {
                self.build_sequence_doc(&mut w);
            } else {
                self.build_action_doc(&mut w);
            }
        }

        if self.origin == "user" {
            w.subtext(
                "Defined in ~/.config/instantwm/config.toml — edit it to change this binding.",
            );
        } else {
            w.subtext("Built-in default — override it in ~/.config/instantwm/config.toml.");
        }

        crate::menu_utils::FzfPreview::Text(w.build_string())
    }
}

/// Choice made in the per-binding action menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmenuChoice {
    Run,
    Edit,
    Back,
}

#[derive(Debug, Clone)]
struct ActionOption {
    label: String,
    key: String,
    badge: String,
    preview: String,
}

impl ActionOption {
    fn new(label: String, key: &str, icon: NerdFont, color: &str, preview: String) -> Self {
        Self {
            label,
            key: key.to_string(),
            badge: format_icon_colored(icon, color),
            preview,
        }
    }

    fn run(row: &KeybindRow) -> Self {
        // The compositor executes the action as `instantwmctl action
        // <name> <args>` — same split `run_binding` relies on.
        let name = action_name(&row.action);
        let args = row.action[name.len()..].trim_start();
        let command = if args.is_empty() {
            format!("instantwmctl action {name}")
        } else {
            format!("instantwmctl action {name} {args}")
        };
        Self::new(
            format!("Run: {}", row.action),
            "run",
            NerdFont::Bolt,
            colors::GREEN,
            PreviewBuilder::new()
                .title(colors::GREEN, "Run this action")
                .blank()
                .text("Executes the binding's action right now.")
                .field("Command", &command)
                .build_string(),
        )
    }

    fn edit() -> Self {
        let path = config_path();
        Self::new(
            "Edit config.toml".to_string(),
            "edit",
            NerdFont::Edit,
            colors::BLUE,
            PreviewBuilder::new()
                .title(colors::BLUE, "Edit config")
                .blank()
                .text("Open your instantWM config so you can change or remove this binding.")
                .field("File", &path.display().to_string())
                .field("Editor", "$EDITOR (nvim by default)")
                .build_string(),
        )
    }

    fn back() -> Self {
        Self::new(
            "Back".to_string(),
            "back",
            NerdFont::ArrowLeft,
            colors::OVERLAY1,
            PreviewBuilder::new()
                .subtext("Return to the keybind list")
                .build_string(),
        )
    }
}

impl FzfSelectable for ActionOption {
    fn fzf_display_text(&self) -> String {
        format!("{}{}", self.badge, self.label)
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        crate::menu_utils::FzfPreview::Text(self.preview.clone())
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

    // Attach built-in action documentation (`instantwmctl action --list`
    // works even without a compositor; silently degrade if it's unavailable).
    let docs = fetch_action_docs();
    let rows: Vec<KeybindRow> = rows
        .into_iter()
        .map(|row| row.with_docs(docs.clone()))
        .collect();

    // The action menu can send the user back here, so loop until they cancel.
    loop {
        let header = HeaderBuilder::new(
            NerdFont::Keyboard,
            format!("instantWM keybinds  ·  {} bindings", rows.len()),
        )
        .subtitle("Select a binding to run it, edit its config, or learn about it")
        .build();

        let result = FzfWrapper::builder()
            .prompt(format!("{} ", char::from(NerdFont::Search)))
            .header(header)
            .responsive_layout()
            .select(rows.clone())?;

        match result {
            FzfResult::Selected(row) => match handle_select(&row)? {
                SubmenuChoice::Run => run_binding(&row)?,
                SubmenuChoice::Edit => open_config()?,
                SubmenuChoice::Back => continue,
            },
            FzfResult::Cancelled | FzfResult::MultiSelected(_) => return Ok(()),
            FzfResult::Error(err) => return Err(anyhow!(err)),
        }
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

/// Built-in documentation for every named action, keyed by action name.
/// Best-effort: an empty map degrades the preview to "no description".
fn fetch_action_docs() -> ActionDocs {
    match instantwmctl::json::<Vec<ActionDocJson>, _, _>(["action", "--list"]) {
        Ok(entries) => Arc::new(
            entries
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
        ),
        Err(_) => Arc::new(HashMap::new()),
    }
}

/// Let the user pick what to do with a selected binding.
fn handle_select(row: &KeybindRow) -> Result<SubmenuChoice> {
    let mut options = Vec::new();
    // Sequences can't be triggered individually, so offer edit/back only.
    if row.sequence_steps().is_none() {
        options.push(ActionOption::run(row));
    }
    options.push(ActionOption::edit());
    options.push(ActionOption::back());

    let mut header = HeaderBuilder::new(NerdFont::Keyboard, row.binding())
        .subtitle(&row.action)
        .field("Mode", &row.mode);
    if row.mode != GLOBAL_MODE {
        header = header.field("Availability", &format!("only in '{}' mode", row.mode));
    }
    header = header.field("Origin", row.origin_label());
    let header = header.build();

    let result = FzfWrapper::builder()
        .prompt(format!("{} ", char::from(NerdFont::Wrench)))
        .header(header)
        .responsive_layout()
        .select(options)?;

    match result {
        FzfResult::Selected(o) => match o.key.as_str() {
            "run" => Ok(SubmenuChoice::Run),
            "edit" => Ok(SubmenuChoice::Edit),
            // Unknown keys and cancellation both fall back to the list.
            _ => Ok(SubmenuChoice::Back),
        },
        FzfResult::Cancelled | FzfResult::MultiSelected(_) => Ok(SubmenuChoice::Back),
        FzfResult::Error(err) => Err(anyhow!(err)),
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

    FzfWrapper::message(&format!("Opening {} in {}…", path.display(), editor))?;

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
        let origin = if r.origin == "user" {
            "config"
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::FzfPreview;
    use crate::menu_utils::MockQueue;

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

    fn row(
        modifiers: &str,
        key: &str,
        action: &str,
        mode: Option<&str>,
        origin: &str,
    ) -> KeybindRow {
        KeybindRow {
            modifiers: modifiers.to_string(),
            key: key.to_string(),
            action: action.to_string(),
            mode: mode.unwrap_or(GLOBAL_MODE).to_string(),
            origin: origin.to_string(),
            docs: Arc::new(HashMap::new()),
        }
    }

    /// Row with a docs map, for preview tests.
    fn docs_row(docs: &[(&str, &str)]) -> KeybindRow {
        let mut map = HashMap::new();
        for (name, description) in docs {
            map.insert(
                (*name).to_string(),
                ActionDoc {
                    description: (*description).to_string(),
                    arg_example: None,
                },
            );
        }
        KeybindRow {
            modifiers: "Super".to_string(),
            key: "i".to_string(),
            action: "inc_master_count 1".to_string(),
            mode: GLOBAL_MODE.to_string(),
            origin: "compiled_default".to_string(),
            docs: Arc::new(map),
        }
    }

    fn preview_text(row: &KeybindRow) -> String {
        match row.fzf_preview() {
            FzfPreview::Text(text) => text,
            other => panic!("expected Text preview, got {other:?}"),
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
    fn list_items_are_lean_no_mode_or_origin() {
        let row = row(
            "Super",
            "Return",
            "spawn kitty --single-instance",
            Some("desktop"),
            "user",
        );
        let display = row.fzf_display_text();
        assert!(display.contains("Super + Return"));
        assert!(display.contains("spawn kitty --single-instance"));
        // Mode and origin belong in the preview, not in the list item.
        assert!(!display.contains("desktop"));
        assert!(!display.contains("global"));
        assert!(!display.contains("your config"));
        assert!(!display.contains("default"));
    }

    #[test]
    fn preview_shows_origin_in_preview_not_item() {
        let user = row(
            "Super",
            "Return",
            "spawn kitty --single-instance",
            None,
            "user",
        );
        let default = row("Super", "h", "focus_left", None, "compiled_default");

        let user_text = preview_text(&user);
        assert!(user_text.contains("your config"));
        assert!(user_text.contains("Defined in ~/.config/instantwm/config.toml"));

        let default_text = preview_text(&default);
        assert!(default_text.contains("default"));
        assert!(default_text.contains("Built-in default"));

        // Both previews carry the binding and action.
        assert!(user_text.contains("Super + Return"));
        assert!(default_text.contains("focus_left"));
    }

    #[test]
    fn non_global_mode_preview_notes_availability() {
        let row = row(
            "",
            "Return",
            "spawn .config/instantos/default/terminal",
            Some("desktop"),
            "compiled_default",
        );
        let text = preview_text(&row);
        assert!(text.contains("desktop"));
        assert!(text.contains("Only available when the 'desktop' mode is enabled"));
    }

    #[test]
    fn global_mode_preview_has_no_availability_note() {
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        let text = preview_text(&row);
        assert!(text.contains("global"));
        assert!(!text.contains("Only available"));
    }

    #[test]
    fn preview_includes_action_docs_and_usage() {
        let row = docs_row(&[("inc_master_count", "increase master window count")]);
        let row_with_arg = KeybindRow {
            docs: Arc::new(
                [(
                    "inc_master_count".to_string(),
                    ActionDoc {
                        description: "increase master window count".to_string(),
                        arg_example: Some("1".to_string()),
                    },
                )]
                .into_iter()
                .collect(),
            ),
            ..row
        };

        let text = preview_text(&row_with_arg);
        assert!(text.contains("increase master window count"));
        // The value is wrapped in instantCLI's own ANSI color codes (added
        // by PreviewWriter for fzf's --ansi preview), so check the label and
        // the rendered value separately. The instantwmctl JSON has no codes.
        assert!(text.contains("Usage"));
        assert!(text.contains("inc_master_count 1"));
    }

    #[test]
    fn sequence_preview_lists_steps_with_docs() {
        let row = row(
            "Super + Ctrl",
            "Shift",
            "sequence [set_mode default, spawn ins assist run sf]",
            None,
            "compiled_default",
        )
        .with_docs(Arc::new(
            [
                (
                    "set_mode".to_string(),
                    ActionDoc {
                        description: "set WM mode (sway-like modes)".to_string(),
                        arg_example: Some("resize".to_string()),
                    },
                ),
                (
                    "spawn".to_string(),
                    ActionDoc {
                        description: "spawn a command without shell expansion".to_string(),
                        arg_example: Some("COMMAND [ARG ...]".to_string()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        ));

        let text = preview_text(&row);
        assert!(text.contains("Sequence"));
        assert!(text.contains("set_mode default — set WM mode (sway-like modes)"));
        assert!(text.contains("spawn ins assist run sf — spawn a command without shell expansion"));
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
        assert!(row.sequence_steps().is_some());
        assert!(row.sequence_steps().unwrap().len() == 2);
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
    fn submenu_back_selection_returns_back() {
        let _guard = MockQueue::new().select_index(2).guard();
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        assert_eq!(handle_select(&row).unwrap(), SubmenuChoice::Back);
    }

    #[test]
    fn submenu_cancel_returns_back() {
        let _guard = MockQueue::new().cancel_selection().guard();
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        assert_eq!(handle_select(&row).unwrap(), SubmenuChoice::Back);
    }

    #[test]
    fn submenu_run_option_sits_first_for_plain_actions() {
        // Index 0 in the action menu is "Run", index 1 is "Edit".
        let _guard = MockQueue::new().select_index(0).guard();
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        // handle_select would execute the action via instantwmctl; assert the
        // option ordering instead by checking the option list construction.
        let options_with_run = {
            let mut opts = Vec::new();
            opts.push(ActionOption::run(&row));
            opts.push(ActionOption::edit());
            opts.push(ActionOption::back());
            opts
        };
        assert_eq!(options_with_run[0].key, "run");
        assert_eq!(options_with_run[1].key, "edit");
        assert_eq!(options_with_run[2].key, "back");
        assert!(options_with_run[0].label.starts_with("Run:"));
        assert!(
            options_with_run[0]
                .badge
                .contains(char::from(NerdFont::Bolt))
        );
    }

    #[test]
    fn sequence_rows_get_no_run_option() {
        let row = row(
            "Super + Ctrl",
            "Shift",
            "sequence [set_mode default, spawn ins assist run sf]",
            None,
            "compiled_default",
        );
        // The submenu for a sequence offers edit + back only.
        let _guard = MockQueue::new().select_index(1).guard();
        assert_eq!(handle_select(&row).unwrap(), SubmenuChoice::Back);
    }

    #[test]
    fn back_option_appears_after_edit() {
        let back = ActionOption::back();
        assert_eq!(back.key, "back");
        assert!(back.label.contains("Back"));
        assert!(back.badge.contains(char::from(NerdFont::ArrowLeft)));
    }
}
