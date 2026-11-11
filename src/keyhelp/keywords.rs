use super::GLOBAL_MODE;

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
