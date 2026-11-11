use super::GLOBAL_MODE;
use super::derive_keywords;
use super::menu::{SubmenuAction, SubmenuActionItem, handle_select};
use super::model::{ActionDoc, KeybindPreviewPayload, KeybindRow, KeybindRowJson};
use super::ranking::sort_keybinds;
use crate::menu_utils::FzfPreview;
use crate::menu_utils::FzfSelectable;
use crate::menu_utils::MockQueue;
use crate::ui::catppuccin::{colors, format_with_color};
use crate::ui::nerd_font::NerdFont;
use std::collections::HashMap;

/// Rows captured from `instantwmctl --json keybinds` on a real instantWM
/// session. Covers every origin value, all four modes, and a mix of
/// simple actions, args, spawns, and sequences.
const SAMPLE_JSON: &str = include_str!("../../tests/fixtures/keybinds.json");

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
