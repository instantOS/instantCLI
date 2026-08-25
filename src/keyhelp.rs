//! `ins keyhelp` - explore and memorize instantWM keybinds.
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
/// done by the `ins preview --id keyhelp` child process on highlight - see
/// [`KeybindPreviewPayload`] and `crate::preview::keyhelp::render_keyhelp_preview`.
#[derive(Debug, Clone)]
struct KeybindRow {
    modifiers: String,
    key: String,
    action: String,
    mode: String,
    origin: String,
    keywords: Vec<&'static str>,
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
            "built-in default"
        }
    }

    /// Build the full preview text with prominent action description,
    /// structured Details section, and origin explanation.
    pub fn render_preview(&self, docs: &HashMap<String, ActionDoc>) -> String {
        let mut w = PreviewWriter::collect();

        // 1. Header with the keybinding chord
        w.header(NerdFont::Keyboard, &self.binding());

        // 2. Action description right under the header (primary information)
        if self.sequence_steps().is_some() {
            self.build_sequence_doc(docs, &mut w);
        } else if !docs.is_empty() {
            self.build_action_doc(docs, &mut w);
        }

        w.blank();
        w.separator();
        w.blank();

        // 3. Structured Details section
        w.title(colors::BLUE, "Details");
        w.field("Action", &self.action);

        let name = action_name(&self.action);
        if let Some(doc) = docs.get(name)
            && let Some(arg) = &doc.arg_example
        {
            w.field("Usage", &format!("{name} {arg}"));
        }

        if self.mode == GLOBAL_MODE {
            w.field("Mode", "global (all modes)");
        } else {
            w.field("Mode", &format!("{} mode", self.mode));
            w.line(
                colors::MAUVE,
                Some(NerdFont::Info),
                &format!("Only available when '{}' mode is active", self.mode),
            );
        }

        w.field("Origin", self.origin_label());
        w.blank();

        // 4. Clear footer explanation
        if self.origin == "user" {
            w.subtext(
                "Defined in ~/.config/instantwm/config.toml. Select 'Edit config' to change it.",
            );
        } else {
            w.subtext("Built-in default. You can override it in ~/.config/instantwm/config.toml.");
        }

        w.build_string()
    }

    fn build_action_doc(&self, docs: &HashMap<String, ActionDoc>, builder: &mut PreviewWriter) {
        let name = action_name(&self.action);
        match docs.get(name) {
            Some(doc) => {
                builder.line(colors::SKY, Some(NerdFont::Info), &doc.description);
            }
            None => {
                builder.subtext(&format!("Executes `{name}`."));
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
                            builder.bullet(&format!("{step}: {}", doc.description));
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
        let mode = j.mode.unwrap_or_else(|| GLOBAL_MODE.to_string());
        let keywords = derive_keywords(&j.action, &mode);
        Self {
            modifiers: j.modifiers,
            key: j.key,
            action: j.action,
            mode,
            origin: j.origin,
            keywords,
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

/// Derive search keywords from action and mode to help users find bindings by intent/synonym.
pub fn derive_keywords(action: &str, mode: &str) -> Vec<&'static str> {
    let mut kw = Vec::new();
    let lower = action.to_ascii_lowercase();

    // Application spawns & system tools
    if lower.contains("default/terminal")
        || lower.contains("kitty")
        || lower.contains("st")
        || lower.ends_with("terminal")
    {
        kw.extend_from_slice(&["terminal", "console", "shell", "cli", "kitty"]);
    }
    if lower.contains("termfilemanager") {
        kw.extend_from_slice(&["termfilemanager", "ranger", "yazi", "nnn"]);
    } else if lower.contains("filemanager") {
        kw.extend_from_slice(&["file manager", "files", "explorer", "directory", "folders"]);
    }
    if lower.contains("appmenu") || lower.contains("dmenu") || lower.contains("smart") {
        kw.extend_from_slice(&[
            "app launcher",
            "applications",
            "app menu",
            "dmenu",
            "rofi",
            "apps",
        ]);
    }
    if lower.contains("browser") {
        kw.extend_from_slice(&[
            "web browser",
            "internet",
            "firefox",
            "chrome",
            "chromium",
            "web",
        ]);
    }
    if lower.contains("editor") {
        kw.extend_from_slice(&["text editor", "code", "nvim", "nano", "vim"]);
    }
    if lower.contains("systemmonitor") || lower.contains("htop") || lower.contains("btop") {
        kw.extend_from_slice(&[
            "task manager",
            "system monitor",
            "processes",
            "activity monitor",
            "resources",
            "cpu",
            "ram",
        ]);
    }
    if lower.contains("lockscreen") || lower.contains("hyprlock") || lower.contains("instantlock") {
        kw.extend_from_slice(&["lock screen", "screenlock", "lock"]);
    }
    if lower.contains("settings") {
        kw.extend_from_slice(&["settings", "control center", "preferences", "config"]);
    }
    if lower.contains("keyhelp") {
        kw.extend_from_slice(&[
            "keyhelp",
            "shortcuts",
            "hotkeys",
            "keyboard shortcuts",
            "cheat sheet",
            "help",
        ]);
    }
    if lower.contains("assist") {
        kw.extend_from_slice(&["assist", "assistant", "quick actions", "tools"]);
    }
    if lower.contains("shutdown") || lower.contains("instantshutdown") {
        kw.extend_from_slice(&[
            "power off",
            "shutdown",
            "reboot",
            "restart",
            "logout",
            "exit session",
            "sleep",
        ]);
    }
    if lower.contains("search") || lower.contains("instantsearch") {
        kw.extend_from_slice(&["web search", "find online", "google"]);
    }
    if lower.contains("clip") {
        kw.extend_from_slice(&["clipboard", "clipmenu", "copy", "paste"]);
    }
    if lower.contains("iswitch")
        || lower.contains("rofi_window_switch")
        || lower.contains("window_switch")
    {
        kw.extend_from_slice(&["alt tab", "switch window", "window switcher", "tasks"]);
    }
    if lower.contains("screenshot") || lower.contains("print") {
        kw.extend_from_slice(&["screenshot", "screen capture", "snip", "flameshot"]);
    }
    if lower.contains("vol")
        || lower.contains("volume")
        || lower.contains("mute")
        || lower.contains("audio")
    {
        kw.extend_from_slice(&["volume control", "audio", "sound", "mute", "mic"]);
    }
    if lower.contains("bright") {
        kw.extend_from_slice(&["brightness", "backlight", "dim screen", "screen brightness"]);
    }
    if lower.contains("playerctl") || lower.contains("music") || lower.contains("play-pause") {
        kw.extend_from_slice(&[
            "music player",
            "playback",
            "media control",
            "play pause",
            "song",
            "spotify",
        ]);
    }

    // Window management actions
    if lower.contains("focus") {
        kw.extend_from_slice(&["focus window", "navigate windows", "select window"]);
    }
    if lower.contains("move") || lower.contains("push") || lower.contains("swap") {
        kw.extend_from_slice(&["move window", "rearrange", "swap positions"]);
    }
    if lower.contains("resize") || lower.contains("grow") || lower.contains("shrink") {
        kw.extend_from_slice(&["resize window", "expand", "shrink", "dimensions"]);
    }
    if lower.contains("layout") || lower.contains("grid") || lower.contains("monocle") {
        kw.extend_from_slice(&["layout switch", "tiling mode", "grid layout"]);
    }
    if lower.contains("float") {
        kw.extend_from_slice(&["floating window", "toggle float", "unfloat"]);
    }
    if lower.contains("maximize") || lower.contains("zoom") {
        kw.extend_from_slice(&[
            "maximize window",
            "fullscreen",
            "toggle fullscreen",
            "zoom master",
        ]);
    }
    if lower.contains("sticky") {
        kw.extend_from_slice(&[
            "sticky window",
            "pin window",
            "show on all workspaces",
            "pin",
        ]);
    }
    if lower.contains("overview") || lower.contains("win_view") {
        kw.extend_from_slice(&["window overview", "expose", "mission control", "grid view"]);
    }
    if lower.contains("scratchpad") {
        kw.extend_from_slice(&[
            "scratchpad",
            "dropdown",
            "drawer",
            "quake",
            "toggle scratchpad",
        ]);
    }
    if lower.contains("bar") {
        kw.extend_from_slice(&["status bar", "taskbar", "panel", "toggle bar"]);
    }
    if lower.contains("keyboard") || lower.contains("layout") {
        kw.extend_from_slice(&["keyboard language", "switch layout", "xkb"]);
    }
    if lower.contains("hide") || lower.contains("unhide") {
        kw.extend_from_slice(&["minimize window", "hide window", "unhide windows"]);
    }
    if lower.contains("kill") || lower.contains("shut_kill") {
        kw.extend_from_slice(&["close window", "kill application", "close app", "terminate"]);
    }
    if lower == "quit" {
        kw.extend_from_slice(&["exit instantwm", "logout"]);
    }
    if lower.contains("tag") || lower.contains("view") {
        kw.extend_from_slice(&["workspace", "virtual desktop", "tag"]);
    }
    if lower.contains("mon") {
        kw.extend_from_slice(&["monitor", "screen switch", "display"]);
    }

    if mode != GLOBAL_MODE {
        match mode {
            "desktop" => kw.extend_from_slice(&["desktop mode"]),
            "placement" => kw.extend_from_slice(&["placement mode"]),
            "prefix" => kw.extend_from_slice(&["prefix mode"]),
            "overview" => kw.extend_from_slice(&["overview mode"]),
            _ => {}
        }
    }

    kw.dedup();
    kw
}

/// Heuristic priority for keybindings in the initial menu view.
/// Returns `(tier, sub_priority)`.
///
/// Tiers:
/// - 0: User-defined bindings (from `config.toml`)
/// - 1: Core application spawns and primary launchers (terminal, app menu, keyhelp, filemanager, browser, settings...)
/// - 2: Essential window lifecycle & state toggles (kill/close, alt-tab switcher, maximize, float, scratchpad, overview...)
/// - 3: Navigation & Focus (focus directions, monitor switching, tag back/forth)
/// - 4: Window movement, resizing & layout selection (move, resize, split ratio, layout cycling)
/// - 5: Workspaces & Tags (view tag, set tag, toggle tag)
/// - 6: Hardware, media, screenshots & system controls (volume, brightness, media, screenshot, power...)
/// - 7: Modal/secondary modes (desktop, placement, prefix, overview modes)
fn keybind_tier(row: &KeybindRow) -> (u8, u8) {
    if row.origin == "user" {
        return (0, 0);
    }

    if row.mode != GLOBAL_MODE {
        return (7, 0);
    }

    let lower = row.action.to_ascii_lowercase();

    // Tier 1: Core application spawns and primary system launchers
    if lower.contains("default/terminal")
        || lower.contains("kitty")
        || lower.contains("st")
        || lower.ends_with("terminal")
    {
        return (1, 0);
    }
    if lower.contains("appmenu") || lower.contains("smart") || lower.contains("dmenu") {
        return (1, 1);
    }
    if lower.contains("keyhelp") {
        return (1, 2);
    }
    if lower.contains("assist") {
        return (1, 3);
    }
    if lower.contains("filemanager") || lower.contains("termfilemanager") {
        return (1, 4);
    }
    if lower.contains("browser") {
        return (1, 5);
    }
    if lower.contains("editor") {
        return (1, 6);
    }
    if lower.contains("settings") {
        return (1, 7);
    }
    if lower.contains("search") || lower.contains("clip") {
        return (1, 8);
    }

    // Tier 2: Essential window lifecycle & state toggles
    if lower.contains("shut_kill") || lower == "kill" {
        return (2, 0);
    }
    if lower.contains("iswitch") || lower.contains("rofi_window_switch") {
        return (2, 1);
    }
    if lower.contains("tiling_maximized") || lower.contains("fullscreen") || lower == "zoom" {
        return (2, 2);
    }
    if lower.contains("float") {
        return (2, 3);
    }
    if lower.contains("scratchpad") {
        return (2, 4);
    }
    if lower.contains("overview") || lower.contains("win_view") {
        return (2, 5);
    }
    if lower.contains("sticky") {
        return (2, 6);
    }
    if lower.contains("hide") || lower.contains("unhide") {
        return (2, 7);
    }

    // Tier 3: Navigation & Focus
    if lower.starts_with("focus_") || lower.contains("focus") {
        return (3, 0);
    }
    if lower == "last_view" || lower == "follow_view" {
        return (3, 1);
    }
    if lower.contains("focus_mon") || lower.contains("follow_mon") {
        return (3, 2);
    }

    // Tier 4: Window movement, resizing & layout selection
    if lower.starts_with("key_move")
        || lower.contains("move_client")
        || lower.contains("push")
        || lower.contains("swap")
    {
        return (4, 0);
    }
    if lower.starts_with("key_resize")
        || lower.contains("tree_grow")
        || lower.contains("tree_shrink")
    {
        return (4, 1);
    }
    if lower.contains("layout") || lower.contains("inc_master") {
        return (4, 2);
    }

    // Tier 5: Workspaces & Tags
    if lower.contains("tag") || lower.contains("view_all") {
        return (5, 0);
    }

    // Tier 6: Hardware, media, screenshots & system controls
    if lower.contains("vol")
        || lower.contains("mute")
        || lower.contains("bright")
        || lower.contains("playerctl")
    {
        return (6, 0);
    }
    if lower.contains("screenshot") || lower.contains("print") {
        return (6, 1);
    }
    if lower.contains("lockscreen")
        || lower.contains("systemmonitor")
        || lower.contains("shutdown")
        || lower == "quit"
    {
        return (6, 2);
    }

    // Fallback for any other action
    (6, 9)
}

/// Sort keybindings stably using the heuristic priority ranking.
fn sort_keybinds(rows: &mut [KeybindRow]) {
    rows.sort_by_key(keybind_tier);
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

    fn fzf_search_keywords(&self) -> &[&str] {
        &self.keywords
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

/// Dispatch entry point for `ins keyhelp` with `--gui` support.
pub fn run_keyhelp_command(gui: bool, debug: bool) -> Result<()> {
    if gui {
        return launch_keyhelp_in_terminal(debug);
    }
    run_keyhelp()
}

/// Launch `ins keyhelp` in a dedicated GUI terminal window.
pub fn launch_keyhelp_in_terminal(debug: bool) -> Result<()> {
    let mut args: Vec<String> = vec![];

    if debug {
        args.push("--debug".to_string());
    }

    args.push("keyhelp".to_string());

    let current_exe = std::env::current_exe()?;
    let exe_str = current_exe.to_string_lossy();

    crate::common::terminal::TerminalLauncher::new(exe_str.as_ref())
        .class("ins-keyhelp")
        .title("Keyhelp")
        .args(&args)
        .launch()
}

/// Entry point for `ins keyhelp`.
pub fn run_keyhelp() -> Result<()> {
    if !std::io::stdout().is_terminal() {
        return print_text_list();
    }

    let compositor = CompositorType::detect();
    let (rows, is_hyprland) = match compositor {
        CompositorType::InstantWM => (fetch_keybinds()?, false),
        CompositorType::Hyprland => (fetch_hyprland_keybinds()?, true),
        _ => {
            eprintln!(
                "ins keyhelp: keybind listing is only supported on instantWM and Hyprland \
                 (detected: {}).",
                compositor.name()
            );
            return Ok(());
        }
    };
    if rows.is_empty() {
        if is_hyprland {
            println!("No keybindings found. Is Hyprland running?");
        } else {
            println!("No keybindings found. Is instantWM running?");
        }
        return Ok(());
    }

    // Track the user's place across the action submenu, same pattern as
    // `ins settings` and the notification center. After they pick an
    // action (or hit Back), the next open of the keybind list lands on
    // the same row.
    let mut cursor = MenuCursor::new();
    let header_title = if is_hyprland {
        format!("Hyprland keybinds  ·  {} bindings", rows.len())
    } else {
        format!("instantWM keybinds  ·  {} bindings", rows.len())
    };
    let header_subtitle = if is_hyprland {
        "Select a binding to view it or edit hyprland.lua (descriptions show if set)"
    } else {
        "Select a binding to run it, edit its config, or learn about it"
    };

    loop {
        let header = HeaderBuilder::new(NerdFont::Keyboard, header_title.clone())
            .subtitle(header_subtitle)
            .build();

        let selection = FzfWrapper::builder()
            .prompt(format!("{} ", char::from(NerdFont::Search)))
            .header(header.clone())
            .responsive_layout()
            .presentation(MenuPresentation::Padded)
            .cursor(cursor.initial_index(&rows))
            .select_one(rows.clone())?;

        match selection {
            Some(row) => {
                cursor.update(&row, &rows);
                // Hyprland binds are Lua closures (dispatcher __lua). They can't be
                // triggered via instantwmctl. Offer view/edit only, and show a hint
                // for undocumented binds.
                if is_hyprland {
                    match handle_select_hyprland(&row)? {
                        SubmenuAction::Edit => open_hyprland_config()?,
                        SubmenuAction::Back => continue,
                        SubmenuAction::Run => {
                            crate::menu_utils::FzfWrapper::message(
                                "Hyprland binds can't be triggered from keyhelp. Open hyprland.lua to run or test the dispatcher manually.",
                            )?;
                        }
                    }
                } else {
                    match handle_select(&row)? {
                        SubmenuAction::Run => run_binding(&row)?,
                        SubmenuAction::Edit => open_config()?,
                        SubmenuAction::Back => continue,
                    }
                }
            }
            None => return Ok(()),
        }
    }
}

/// Fetch the live binding list. Fails loudly if instantWM isn't running -
/// keyhelp is about the *running* config, not the file on disk.
fn fetch_keybinds() -> Result<Vec<KeybindRow>> {
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

fn hyprland_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("hypr")
        .join("hyprland.lua")
}

fn fetch_hyprland_keybinds() -> Result<Vec<KeybindRow>> {
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

fn handle_select_hyprland(row: &KeybindRow) -> Result<SubmenuAction> {
    // Hyprland binds are Lua closures (dispatcher __lua). Can't be triggered via instantwmctl.
    // Only offer Edit config / Back, and annotate undocumented binds in the preview.
    let mut options: Vec<SubmenuActionItem> = Vec::new();
    options.push(SubmenuActionItem::edit());
    options.push(SubmenuActionItem::back());

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
        .presentation(MenuPresentation::Padded)
        .select_one(options)?;

    match selection {
        Some(item) => Ok(item.action),
        None => Ok(SubmenuAction::Back),
    }
}

fn open_hyprland_config() -> Result<()> {
    let path = hyprland_config_path();
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

/// Translate the config-style action string back into an `instantwmctl
/// action` call. `describe()` renders actions as `name arg1 arg2 ...`, so the
/// first token is the action name and the rest are its args.
fn run_binding(row: &KeybindRow) -> Result<()> {
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
    let rows = match compositor {
        CompositorType::InstantWM => fetch_keybinds()?,
        CompositorType::Hyprland => fetch_hyprland_keybinds()?,
        _ => {
            eprintln!(
                "ins keyhelp: keybind listing is only supported on instantWM and Hyprland \
                 (detected: {}).",
                compositor.name()
            );
            return Ok(());
        }
    };
    if rows.is_empty() {
        if matches!(compositor, CompositorType::Hyprland) {
            println!("No keybindings found. Is Hyprland running?");
        } else {
            println!("No keybindings found. Is instantWM running?");
        }
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
        } else if r.origin.starts_with("hyprland") {
            r.origin.as_str()
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
        let mode_str = mode.unwrap_or(GLOBAL_MODE).to_string();
        let keywords = derive_keywords(action, &mode_str);
        KeybindRow {
            modifiers: modifiers.to_string(),
            key: key.to_string(),
            action: action.to_string(),
            mode: mode_str,
            origin: origin.to_string(),
            keywords,
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
        assert!(default_text.contains("built-in default"));
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
        assert!(text.contains("Only available when 'desktop' mode is active"));
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
        assert!(text.contains("set_mode default: set WM mode (sway-like modes)"));
        assert!(text.contains("spawn ins assist run sf: spawn a command without shell expansion"));
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

    #[test]
    fn derive_keywords_provides_rich_search_aliases() {
        let terminal = row(
            "Super",
            "Return",
            "spawn .config/instantos/default/terminal",
            None,
            "compiled_default",
        );
        let kw = terminal.fzf_search_keywords();
        assert!(kw.contains(&"terminal"));
        assert!(kw.contains(&"shell"));
        assert!(kw.contains(&"cli"));

        let scratchpad = row("Super", "s", "scratchpad_toggle", None, "compiled_default");
        let kw = scratchpad.fzf_search_keywords();
        assert!(kw.contains(&"scratchpad"));
        assert!(kw.contains(&"dropdown"));
        assert!(!kw.contains(&"terminal"));

        let sysmon = row(
            "Super + Shift",
            "Escape",
            "spawn defaults::SYSTEMMONITOR",
            None,
            "compiled_default",
        );
        let kw = sysmon.fzf_search_keywords();
        assert!(kw.contains(&"task manager"));
        assert!(kw.contains(&"cpu"));

        let desktop_mode = row(
            "",
            "Return",
            "spawn .config/instantos/default/terminal",
            Some("desktop"),
            "compiled_default",
        );
        let kw = desktop_mode.fzf_search_keywords();
        assert!(kw.contains(&"desktop mode"));
    }

    #[test]
    fn keybind_ranking_heuristics_orders_essentials_first() {
        let user_custom = row("Super", "b", "spawn custom_script", None, "user");
        let terminal = row(
            "Super",
            "Return",
            "spawn .config/instantos/default/terminal",
            None,
            "compiled_default",
        );
        let close_win = row("Super", "q", "shut_kill", None, "compiled_default");
        let focus_left = row("Super", "h", "focus_left", None, "compiled_default");
        let resize_down = row(
            "Super + Alt",
            "j",
            "key_resize_down",
            None,
            "compiled_default",
        );
        let tag_1 = row("Super", "1", "view_tag 1", None, "compiled_default");
        let vol_up = row(
            "",
            "XF86AudioRaiseVolume",
            "volume_up",
            None,
            "compiled_default",
        );
        let desktop_mode = row(
            "",
            "Return",
            "spawn terminal",
            Some("desktop"),
            "compiled_default",
        );

        let mut rows = vec![
            desktop_mode.clone(),
            vol_up.clone(),
            tag_1.clone(),
            resize_down.clone(),
            focus_left.clone(),
            close_win.clone(),
            terminal.clone(),
            user_custom.clone(),
        ];

        sort_keybinds(&mut rows);

        assert_eq!(rows[0].action, "spawn custom_script"); // Tier 0 (user)
        assert_eq!(rows[1].action, "spawn .config/instantos/default/terminal"); // Tier 1 (terminal/app)
        assert_eq!(rows[2].action, "shut_kill"); // Tier 2 (close window)
        assert_eq!(rows[3].action, "focus_left"); // Tier 3 (focus)
        assert_eq!(rows[4].action, "key_resize_down"); // Tier 4 (resize)
        assert_eq!(rows[5].action, "view_tag 1"); // Tier 5 (tags)
        assert_eq!(rows[6].action, "volume_up"); // Tier 6 (hardware/vol)
        assert_eq!(rows[7].mode, "desktop"); // Tier 7 (modal mode)
    }
}
