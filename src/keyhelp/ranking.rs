use super::GLOBAL_MODE;
use super::model::KeybindRow;

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
pub(super) fn sort_keybinds(rows: &mut [KeybindRow]) {
    rows.sort_by_key(keybind_tier);
}
