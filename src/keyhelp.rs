//! `ins keyhelp` — explore and memorize instantWM keybinds.
//!
//! Fetches the live binding list over `instantwmctl keybinds --json` and
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

use crate::common::compositor::CompositorType;
use crate::common::instantwmctl;
use crate::menu_utils::{FzfSelectable, FzfWrapper, HeaderBuilder, MenuCursor, MenuPresentation};
use crate::preview::{PreviewId, preview_command};
use crate::ui::catppuccin::{
    colors, format_back_icon, format_bold, format_icon, format_icon_colored, format_with_color,
};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::{PreviewBuilder, PreviewWriter};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

/// Mode tag used for bindings that are active in every mode.
const GLOBAL_MODE: &str = "global";

#[derive(Debug, Clone, Deserialize)]
struct KeybindRowJson {
    modifiers: String,
    key: String,
    action: String,
    mode: Option<String>,
    origin: String,
}

/// One keybinding as it appears in the fzf menu.
///
/// `fzf_preview()` returns a *command* (not text), so per-row preview work is
/// done by the `ins preview --id keyhelp` child process on highlight — see
/// [`KeybindPreviewPayload`] and `crate::preview::keyhelp::render_keyhelp_preview`.
#[derive(Debug, Clone)]
struct KeybindRow {
    modifiers: String,
    key: String,
    action: String,
    mode: String,
    origin: String,
}

/// Serializable snapshot of a keybinding. The fzf `key` field carries this
/// JSON; the preview child deserializes it to render the binding's preview
/// without a second `instantwmctl keybinds` round-trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindPreviewPayload {
    pub modifiers: String,
    pub key: String,
    pub action: String,
    pub mode: String,
    pub origin: String,
}

impl KeybindPreviewPayload {
    /// Render the chord as the user would type it: empty modifiers → just the
    /// key, otherwise `Modifiers + Key`.
    pub fn binding(&self) -> String {
        if self.modifiers.is_empty() {
            self.key.clone()
        } else {
            format!("{} + {}", self.modifiers, self.key)
        }
    }

    /// The individual actions of a `sequence [...]` binding, or `None` for a
    /// plain (non-sequence) action.
    pub fn sequence_steps(&self) -> Option<Vec<String>> {
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
    pub fn origin_label(&self) -> &'static str {
        if self.origin == "user" {
            "your config"
        } else {
            "default"
        }
    }

    /// Build the full preview text. `docs` is the action-name → description
    /// map from `instantwmctl --json action --list` (an empty map is fine —
    /// the preview simply omits the docs section).
    pub fn render_preview(&self, docs: &HashMap<String, ActionDoc>) -> String {
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

        if !docs.is_empty() {
            if self.sequence_steps().is_some() {
                self.build_sequence_doc(docs, &mut w);
            } else {
                self.build_action_doc(docs, &mut w);
            }
        }

        if self.origin == "user" {
            w.subtext(
                "Defined in ~/.config/instantwm/config.toml — edit it to change this binding.",
            );
        } else {
            w.subtext("Built-in default — override it in ~/.config/instantwm/config.toml.");
        }

        w.build_string()
    }

    fn build_action_doc(&self, docs: &HashMap<String, ActionDoc>, builder: &mut PreviewWriter) {
        let name = action_name(&self.action);
        match docs.get(name) {
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

    fn build_sequence_doc(&self, docs: &HashMap<String, ActionDoc>, builder: &mut PreviewWriter) {
        builder.title(colors::BLUE, "Sequence");
        match self.sequence_steps() {
            Some(steps) => {
                for step in &steps {
                    match docs.get(action_name(step)) {
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

/// Documentation for one named WM action, sourced from
/// `instantwmctl --json action --list`.
#[derive(Debug, Clone)]
pub struct ActionDoc {
    pub description: String,
    pub arg_example: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionDocJson {
    name: String,
    description: Option<String>,
    arg_example: Option<String>,
}

impl KeybindRow {
    fn from_json(j: KeybindRowJson) -> Self {
        Self {
            modifiers: j.modifiers,
            key: j.key,
            action: j.action,
            mode: j.mode.unwrap_or_else(|| GLOBAL_MODE.to_string()),
            origin: j.origin,
        }
    }

    /// Snapshot used as the fzf key field — the preview child parses this
    /// back out to render the binding's preview.
    fn to_payload(&self) -> KeybindPreviewPayload {
        KeybindPreviewPayload {
            modifiers: self.modifiers.clone(),
            key: self.key.clone(),
            action: self.action.clone(),
            mode: self.mode.clone(),
            origin: self.origin.clone(),
        }
    }

    /// Convenience wrapper used by [`KeybindPreviewPayload::binding`].
    fn binding(&self) -> String {
        self.to_payload().binding()
    }

    /// Render the chord with bold key tokens and dimmed, unbolded plus separators
    /// so that combination `+` signs are easily distinguished from literal `+` keys.
    fn formatted_binding(&self) -> String {
        let separator = format!(" {} ", format_with_color("+", colors::OVERLAY0));
        let mut tokens: Vec<String> = self
            .modifiers
            .split('+')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(format_bold)
            .collect();

        if !self.key.is_empty() {
            tokens.push(format_bold(&self.key));
        }

        tokens.join(&separator)
    }

    /// Forwarded so the action submenu can keep using `row.sequence_steps()`
    /// without converting to a payload first.
    fn sequence_steps(&self) -> Option<Vec<String>> {
        self.to_payload().sequence_steps()
    }

    /// Forwarded so the action submenu can keep using `row.origin_label()`.
    fn origin_label(&self) -> &'static str {
        self.to_payload().origin_label()
    }
}

/// First token of an action string: the named action (`inc_master_count 1` →
/// `inc_master_count`). The whole string is returned when it has no args.
fn action_name(action: &str) -> &str {
    action.split_whitespace().next().unwrap_or(action)
}

impl FzfSelectable for KeybindRow {
    fn fzf_display_text(&self) -> String {
        // `format_icon` emits its own ANSI reset. Key tokens are bolded with
        // subtle, unbolded plus separators, followed by an arrow separator
        // and the WM action name.
        format!(
            "{} {}  {}  {}",
            format_icon(NerdFont::Keyboard),
            self.formatted_binding(),
            format_with_color("→", colors::OVERLAY0),
            self.action,
        )
    }

    fn fzf_key(&self) -> String {
        // The fzf field is opaque to users but must be unique per row and
        // round-trippable into a [`KeybindPreviewPayload`]. JSON does both.
        serde_json::to_string(&self.to_payload()).unwrap_or_default()
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        // Lazy preview: fzf runs this command on highlight, passing the
        // payload as `$1`. `ins preview --id keyhelp` rebuilds the sectioned
        // preview (mode/origin/availability + action docs) on demand.
        crate::menu_utils::FzfPreview::Command(preview_command(PreviewId::Keyhelp))
    }
}

/// Choice made in the per-binding action menu. Mirrors the systemd
/// submenu shape: a small enum carried by a thin `SubmenuActionItem`
/// wrapper that renders the row + its preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmenuAction {
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
struct SubmenuActionItem {
    action: SubmenuAction,
    run_action: Option<String>,
}

impl SubmenuActionItem {
    fn run(action: String) -> Self {
        Self {
            action: SubmenuAction::Run,
            run_action: Some(action),
        }
    }

    fn edit() -> Self {
        Self {
            action: SubmenuAction::Edit,
            run_action: None,
        }
    }

    fn back() -> Self {
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

    // Track the user's place across the action submenu, same pattern as
    // `ins settings` and the notification center. After they pick an
    // action (or hit Back), the next open of the keybind list lands on
    // the same row.
    let mut cursor = MenuCursor::new();

    loop {
        let header = HeaderBuilder::new(
            NerdFont::Keyboard,
            format!("instantWM keybinds  ·  {} bindings", rows.len()),
        )
        .subtitle("Select a binding to run it, edit its config, or learn about it")
        .build();

        let selection = FzfWrapper::builder()
            .prompt(format!("{} ", char::from(NerdFont::Search)))
            .header(header)
            .responsive_layout()
            .presentation(MenuPresentation::Padded)
            .cursor(cursor.initial_index(&rows))
            .select_one(rows.clone())?;

        match selection {
            Some(row) => {
                cursor.update(&row, &rows);
                match handle_select(&row)? {
                    SubmenuAction::Run => run_binding(&row)?,
                    SubmenuAction::Edit => open_config()?,
                    SubmenuAction::Back => continue,
                }
            }
            None => return Ok(()),
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

/// Let the user pick what to do with a selected binding.
fn handle_select(row: &KeybindRow) -> Result<SubmenuAction> {
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
        .presentation(MenuPresentation::Padded)
        .select_one(options)?;

    match selection {
        Some(item) => Ok(item.action),
        None => Ok(SubmenuAction::Back),
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
        }
    }

    fn payload(
        modifiers: &str,
        key: &str,
        action: &str,
        mode: Option<&str>,
        origin: &str,
    ) -> KeybindPreviewPayload {
        KeybindPreviewPayload {
            modifiers: modifiers.to_string(),
            key: key.to_string(),
            action: action.to_string(),
            mode: mode.unwrap_or(GLOBAL_MODE).to_string(),
            origin: origin.to_string(),
        }
    }

    fn docs(pairs: &[(&str, &str)]) -> HashMap<String, ActionDoc> {
        let mut map = HashMap::new();
        for (name, description) in pairs {
            map.insert(
                (*name).to_string(),
                ActionDoc {
                    description: (*description).to_string(),
                    arg_example: None,
                },
            );
        }
        map
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
        assert!(display.contains("Super"));
        assert!(display.contains("Return"));
        assert!(display.contains("+"));
        assert!(display.contains("spawn kitty --single-instance"));
        assert!(display.contains("→"));
        // Bold chord tokens
        assert!(display.contains("\x1b[1m"));
        assert!(display.contains("\x1b[22m"));
        // Mode and origin belong in the preview, not in the list item.
        assert!(!display.contains("desktop"));
        assert!(!display.contains("global"));
        assert!(!display.contains("your config"));
        assert!(!display.contains("default"));
    }

    #[test]
    fn plus_sign_separators_are_dimmed_and_keys_are_bold() {
        let row = row("Super + Shift", "+", "zoom_in", None, "user");
        let display = row.fzf_display_text();
        // The literal key '+' is bolded:
        assert!(display.contains("\x1b[1m+\x1b[22m"));
        // The combination '+' separator uses the subtle OVERLAY0 color:
        let dimmed_plus = format_with_color("+", colors::OVERLAY0);
        assert!(display.contains(&dimmed_plus));
        // All key tokens are bold:
        assert!(display.contains("\x1b[1mSuper\x1b[22m"));
        assert!(display.contains("\x1b[1mShift\x1b[22m"));
    }

    #[test]
    fn fzf_key_round_trips_into_payload() {
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        let key = row.fzf_key();
        let parsed: KeybindPreviewPayload =
            serde_json::from_str(&key).expect("fzf_key must be valid JSON");
        assert_eq!(parsed.modifiers, "Super");
        assert_eq!(parsed.key, "h");
        assert_eq!(parsed.action, "focus_left");
        assert_eq!(parsed.mode, GLOBAL_MODE);
        assert_eq!(parsed.origin, "compiled_default");
    }

    #[test]
    fn fzf_preview_is_a_lazy_command() {
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        // The per-row preview is rendered on demand by `ins preview --id
        // keyhelp`, not baked into the fzf input — keeps the keybind list
        // light even when there are dozens of bindings.
        match row.fzf_preview() {
            FzfPreview::Command(cmd) => {
                assert!(cmd.contains("preview --id keyhelp"));
                assert!(cmd.contains("$1"));
            }
            other => panic!("expected Command preview, got {other:?}"),
        }
    }

    #[test]
    fn preview_shows_origin_in_preview_not_item() {
        let user = payload(
            "Super",
            "Return",
            "spawn kitty --single-instance",
            None,
            "user",
        );
        let default = payload("Super", "h", "focus_left", None, "compiled_default");

        let user_text = user.render_preview(&HashMap::new());
        assert!(user_text.contains("your config"));
        assert!(user_text.contains("Defined in ~/.config/instantwm/config.toml"));

        let default_text = default.render_preview(&HashMap::new());
        assert!(default_text.contains("default"));
        assert!(default_text.contains("Built-in default"));

        // Both previews carry the binding and action.
        assert!(user_text.contains("Super + Return"));
        assert!(default_text.contains("focus_left"));
    }

    #[test]
    fn non_global_mode_preview_notes_availability() {
        let row = payload(
            "",
            "Return",
            "spawn .config/instantos/default/terminal",
            Some("desktop"),
            "compiled_default",
        );
        let text = row.render_preview(&HashMap::new());
        assert!(text.contains("desktop"));
        assert!(text.contains("Only available when the 'desktop' mode is enabled"));
    }

    #[test]
    fn global_mode_preview_has_no_availability_note() {
        let row = payload("Super", "h", "focus_left", None, "compiled_default");
        let text = row.render_preview(&HashMap::new());
        assert!(text.contains("global"));
        assert!(!text.contains("Only available"));
    }

    #[test]
    fn preview_includes_action_docs_and_usage() {
        let mut docs = docs(&[("inc_master_count", "increase master window count")]);
        docs.insert(
            "inc_master_count".to_string(),
            ActionDoc {
                description: "increase master window count".to_string(),
                arg_example: Some("1".to_string()),
            },
        );

        let row = payload("Super", "i", "inc_master_count 1", None, "compiled_default");
        let text = row.render_preview(&docs);
        assert!(text.contains("increase master window count"));
        // The value is wrapped in instantCLI's own ANSI color codes (added
        // by PreviewWriter for fzf's --ansi preview), so check the label and
        // the rendered value separately. The instantwmctl JSON has no codes.
        assert!(text.contains("Usage"));
        assert!(text.contains("inc_master_count 1"));
    }

    #[test]
    fn sequence_preview_lists_steps_with_docs() {
        let docs = docs(&[
            ("set_mode", "set WM mode (sway-like modes)"),
            ("spawn", "spawn a command without shell expansion"),
        ]);

        let row = payload(
            "Super + Ctrl",
            "Shift",
            "sequence [set_mode default, spawn ins assist run sf]",
            None,
            "compiled_default",
        );

        let text = row.render_preview(&docs);
        assert!(text.contains("Sequence"));
        assert!(text.contains("set_mode default — set WM mode (sway-like modes)"));
        assert!(text.contains("spawn ins assist run sf — spawn a command without shell expansion"));
    }

    #[test]
    fn sequence_actions_are_detected() {
        let row = payload(
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
        assert_eq!(handle_select(&row).unwrap(), SubmenuAction::Back);
    }

    #[test]
    fn submenu_cancel_returns_back() {
        let _guard = MockQueue::new().cancel_selection().guard();
        let row = row("Super", "h", "focus_left", None, "compiled_default");
        assert_eq!(handle_select(&row).unwrap(), SubmenuAction::Back);
    }

    #[test]
    fn submenu_uses_proper_icons_and_labels() {
        let run = SubmenuActionItem::run("focus_left".to_string());
        let edit = SubmenuActionItem::edit();
        let back = SubmenuActionItem::back();

        let run_display = run.fzf_display_text();
        assert!(run_display.contains("Run"));
        assert!(run_display.contains(char::from(NerdFont::Play)));

        let edit_display = edit.fzf_display_text();
        assert!(edit_display.contains("Edit config"));
        assert!(edit_display.contains(char::from(NerdFont::Edit)));

        let back_display = back.fzf_display_text();
        assert!(back_display.contains("Back"));
        assert!(back_display.contains(char::from(NerdFont::ArrowLeft)));
    }

    #[test]
    fn submenu_keys_are_stable_short_strings() {
        // fzf matches items via these keys, so they must be unique and
        // round-trip cleanly through the wrapper enum.
        assert_eq!(SubmenuActionItem::run("x".into()).fzf_key(), "run");
        assert_eq!(SubmenuActionItem::edit().fzf_key(), "edit");
        assert_eq!(SubmenuActionItem::back().fzf_key(), "back");
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
        // The submenu for a sequence offers edit + back only. Index 1 is
        // therefore Back, not Edit — verify the menu shrinks correctly.
        let _guard = MockQueue::new().select_index(1).guard();
        assert_eq!(handle_select(&row).unwrap(), SubmenuAction::Back);
    }
}
