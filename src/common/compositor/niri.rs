use super::{ScratchpadProvider, ScratchpadWindowInfo, create_terminal_process};
use crate::scratchpad::{config::ScratchpadConfig, terminal::Terminal};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub struct Niri;

impl ScratchpadProvider for Niri {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        let current_workspace = focused_workspace()?;

        if let Some(window) = scratchpad_window(&config.window_class())? {
            if window.workspace_id != current_workspace.id {
                move_window_to_workspace(
                    window.id,
                    &workspace_reference(&current_workspace),
                    false,
                )?;
            }

            position_scratchpad_window(&window, config)?;
            focus_window(window.id)?;
            return Ok(());
        }

        create_and_show_scratchpad(config, &current_workspace)
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        let Some(window) = scratchpad_window(&config.window_class())? else {
            return Ok(());
        };

        ensure_hidden_workspace_exists(&config.workspace_name())?;
        move_window_to_workspace(window.id, &config.workspace_name(), false)
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        if self.is_visible(config)? {
            self.hide(config)
        } else {
            self.show(config)
        }
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        let windows = windows()?;
        let workspaces = workspaces()?;

        Ok(windows
            .into_iter()
            .filter_map(|window| {
                let name = scratchpad_name(window.app_id.as_deref())?;
                let visible = workspaces
                    .iter()
                    .find(|workspace| workspace.id == window.workspace_id)
                    .is_some_and(|workspace| workspace.is_focused);

                Some(ScratchpadWindowInfo {
                    name: name.to_string(),
                    window_class: window.app_id.unwrap_or_default(),
                    title: window.title,
                    visible,
                })
            })
            .collect())
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        Ok(scratchpad_window(&config.window_class())?.is_some())
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        let Some(window) = scratchpad_window(&config.window_class())? else {
            return Ok(false);
        };

        Ok(focused_workspace()?.id == window.workspace_id)
    }

    fn supports_scratchpad(&self) -> bool {
        true
    }
}

#[derive(Debug, Deserialize)]
struct KeyboardLayoutsReply {
    names: Vec<String>,
    current_idx: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct NiriWindow {
    id: u64,
    title: String,
    #[serde(default)]
    app_id: Option<String>,
    workspace_id: u64,
    is_floating: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct NiriWorkspace {
    id: u64,
    idx: usize,
    name: Option<String>,
    is_focused: bool,
    active_window_id: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct KeyboardLayoutsState {
    pub names: Vec<String>,
    pub current_idx: usize,
}

pub fn keyboard_layouts() -> Result<KeyboardLayoutsState> {
    let output = Command::new("niri")
        .args(["msg", "--json", "keyboard-layouts"])
        .output()
        .context("Failed to run niri msg --json keyboard-layouts")?;

    if !output.status.success() {
        bail!(
            "niri msg keyboard-layouts failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let reply: KeyboardLayoutsReply = serde_json::from_slice(&output.stdout)
        .context("Failed to parse niri keyboard-layouts JSON")?;

    Ok(KeyboardLayoutsState {
        names: reply.names,
        current_idx: reply.current_idx,
    })
}

pub fn switch_layout(index: usize) -> Result<()> {
    let layout = index.to_string();
    let output = Command::new("niri")
        .args(["msg", "action", "switch-layout", &layout])
        .output()
        .with_context(|| format!("Failed to run niri msg action switch-layout {layout}"))?;

    if !output.status.success() {
        bail!(
            "niri msg action switch-layout {} failed: {}",
            index,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

pub fn reload_config() -> Result<()> {
    let output = Command::new("niri")
        .args(["msg", "action", "load-config-file"])
        .output()
        .context("Failed to run niri msg action load-config-file")?;

    if !output.status.success() {
        bail!(
            "niri msg action load-config-file failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

pub fn current_mouse_speed() -> Result<f64> {
    let config = read_config()?;
    find_property_value(&config, &["input", "mouse"], "accel-speed")
        .or_else(|| find_property_value(&config, &["input", "touchpad"], "accel-speed"))
        .map(|value| {
            value
                .parse::<f64>()
                .context("Invalid niri accel-speed value")
        })
        .transpose()
        .map(|value| value.unwrap_or(0.0))
}

pub fn set_mouse_speed(speed: f64) -> Result<()> {
    let value = trim_float(speed.clamp(-1.0, 1.0));
    let config = read_config()?;
    let config = upsert_property(&config, &["input", "mouse"], "accel-speed", &value);
    let config = upsert_property(&config, &["input", "touchpad"], "accel-speed", &value);
    write_config(&config)?;
    reload_config()
}

pub fn set_mouse_accel_profile(profile: &str) -> Result<()> {
    if !matches!(profile, "adaptive" | "flat") {
        bail!("Unsupported niri accel-profile '{profile}'");
    }

    let config = read_config()?;
    let value = quoted(profile);
    let config = upsert_property(&config, &["input", "mouse"], "accel-profile", &value);
    let config = upsert_property(&config, &["input", "touchpad"], "accel-profile", &value);
    write_config(&config)?;
    reload_config()
}

pub fn set_keyboard_layouts(layouts: &[String]) -> Result<()> {
    let joined = layouts.join(",");
    if joined.trim().is_empty() {
        bail!("No keyboard layouts selected");
    }

    let config = read_config()?;
    let config = remove_property(&config, &["input", "keyboard", "xkb"], "file");
    let config = remove_property(&config, &["input", "keyboard", "xkb"], "variant");
    let config = upsert_property(
        &config,
        &["input", "keyboard", "xkb"],
        "layout",
        &quoted(&joined),
    );
    write_config(&config)?;
    reload_config()?;
    switch_layout(0)
}

pub fn current_keyboard_layout_codes() -> Result<Vec<String>> {
    let config = read_config()?;
    let xkb_path = ["input", "keyboard", "xkb"];

    if let Some(file_path) = find_property_value(&config, &xkb_path, "file") {
        bail!(
            "niri keyboard layout is managed by xkb file '{}'; changing layouts here is not supported",
            file_path
        );
    }

    if let Some(layouts) = find_property_value(&config, &xkb_path, "layout") {
        return Ok(split_csv(&layouts));
    }

    Ok(localectl_x11_layouts().unwrap_or_default())
}

fn windows() -> Result<Vec<NiriWindow>> {
    let output = Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output()
        .context("Failed to run niri msg --json windows")?;

    if !output.status.success() {
        bail!(
            "niri msg windows failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse niri windows JSON")
}

fn workspaces() -> Result<Vec<NiriWorkspace>> {
    let output = Command::new("niri")
        .args(["msg", "--json", "workspaces"])
        .output()
        .context("Failed to run niri msg --json workspaces")?;

    if !output.status.success() {
        bail!(
            "niri msg workspaces failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse niri workspaces JSON")
}

fn focused_workspace() -> Result<NiriWorkspace> {
    workspaces()?
        .into_iter()
        .find(|workspace| workspace.is_focused)
        .context("Unable to determine focused niri workspace")
}

fn scratchpad_window(window_class: &str) -> Result<Option<NiriWindow>> {
    Ok(windows()?
        .into_iter()
        .find(|window| window.app_id.as_deref() == Some(window_class)))
}

fn scratchpad_name(app_id: Option<&str>) -> Option<&str> {
    app_id?.strip_prefix("scratchpad_")
}

fn workspace_reference(workspace: &NiriWorkspace) -> String {
    workspace
        .name
        .clone()
        .unwrap_or_else(|| workspace.idx.to_string())
}

fn ensure_hidden_workspace_exists(name: &str) -> Result<()> {
    let workspaces = workspaces()?;
    if workspaces
        .iter()
        .any(|workspace| workspace.name.as_deref() == Some(name))
    {
        return Ok(());
    }

    let workspace = workspaces
        .iter()
        .find(|workspace| workspace.name.is_none() && workspace.active_window_id.is_none())
        .or_else(|| {
            workspaces
                .iter()
                .find(|workspace| workspace.name.is_none() && !workspace.is_focused)
        })
        .context(
            "niri scratchpad requires at least one empty workspace to reserve as hidden storage",
        )?;

    run_niri_action(&[
        "set-workspace-name",
        "--workspace",
        &workspace.idx.to_string(),
        name,
    ])
}

fn create_and_show_scratchpad(
    config: &ScratchpadConfig,
    current_workspace: &NiriWorkspace,
) -> Result<()> {
    launch_scratchpad_terminal(config).context("Failed to launch niri scratchpad terminal")?;

    let mut attempts = 0;
    while attempts < 40 {
        if let Some(window) = scratchpad_window(&config.window_class())? {
            if window.workspace_id != current_workspace.id {
                move_window_to_workspace(
                    window.id,
                    &workspace_reference(current_workspace),
                    false,
                )?;
            }

            position_scratchpad_window(&window, config)?;
            focus_window(window.id)?;
            return Ok(());
        }

        thread::sleep(Duration::from_millis(150));
        attempts += 1;
    }

    bail!(
        "niri scratchpad window '{}' did not appear after launch",
        config.window_class()
    )
}

fn launch_scratchpad_terminal(config: &ScratchpadConfig) -> Result<()> {
    match &config.terminal {
        Terminal::Kitty => {
            let term_cmd = config
                .terminal_command()
                .replacen("kitty ", "kitty --detach ", 1);
            let bg_cmd = format!("nohup {term_cmd} >/dev/null 2>&1 &");

            Command::new("sh")
                .args(["-c", &bg_cmd])
                .output()
                .context("Failed to launch kitty scratchpad in detached mode")?;

            Ok(())
        }
        _ => create_terminal_process(config),
    }
}

fn position_scratchpad_window(window: &NiriWindow, config: &ScratchpadConfig) -> Result<()> {
    if !window.is_floating {
        run_niri_action(&["move-window-to-floating", "--id", &window.id.to_string()])?;
    }

    set_window_width(window.id, config.width_pct)?;
    set_window_height(window.id, config.height_pct)?;
    center_window(window.id)
}

fn move_window_to_workspace(window_id: u64, workspace: &str, focus: bool) -> Result<()> {
    let focus_value = if focus { "true" } else { "false" };
    run_niri_action(&[
        "move-window-to-workspace",
        "--window-id",
        &window_id.to_string(),
        "--focus",
        focus_value,
        workspace,
    ])
}

fn focus_window(window_id: u64) -> Result<()> {
    run_niri_action(&["focus-window", "--id", &window_id.to_string()])
}

fn center_window(window_id: u64) -> Result<()> {
    run_niri_action(&["center-window", "--id", &window_id.to_string()])
}

fn set_window_width(window_id: u64, pct: u32) -> Result<()> {
    let width = format!("{}%", pct.clamp(5, 100));
    run_niri_action(&["set-window-width", "--id", &window_id.to_string(), &width])
}

fn set_window_height(window_id: u64, pct: u32) -> Result<()> {
    let height = format!("{}%", pct.clamp(5, 100));
    run_niri_action(&["set-window-height", "--id", &window_id.to_string(), &height])
}

fn run_niri_action(args: &[&str]) -> Result<()> {
    let output = Command::new("niri")
        .arg("msg")
        .arg("action")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run niri msg action {}", args.join(" ")))?;

    if !output.status.success() {
        bail!(
            "niri msg action {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

fn trim_float(value: f64) -> String {
    let mut trimmed = format!("{value:.3}");
    while trimmed.contains('.') && trimmed.ends_with('0') {
        trimmed.pop();
    }
    if trimmed.ends_with('.') {
        trimmed.pop();
    }
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed
    }
}

fn config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("NIRI_CONFIG")
        && !path.is_empty()
    {
        return Ok(PathBuf::from(path));
    }

    if let Some(base) = dirs::config_dir() {
        let user_config = base.join("niri").join("config.kdl");
        if user_config.exists() {
            return Ok(user_config);
        }
    }

    let system_config = PathBuf::from("/etc/niri/config.kdl");
    if system_config.exists() {
        return Ok(system_config);
    }

    let base = dirs::config_dir().context("Unable to determine config directory")?;
    Ok(base.join("niri").join("config.kdl"))
}

fn read_config() -> Result<String> {
    let path = config_path()?;
    fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))
}

fn write_config(content: &str) -> Result<()> {
    let path = config_path()?;
    ensure_parent_dir(&path)?;
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
}

fn find_property_value(content: &str, path: &[&str], property: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let block = find_block_lines(&lines, path)?;

    for line in &lines[block.start + 1..block.end] {
        let trimmed = strip_comment(line).trim();
        if let Some(rest) = trimmed.strip_prefix(property) {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.trim_matches('"').to_string());
            }
        }
    }

    None
}

fn remove_property(content: &str, path: &[&str], property: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
    let Some(range) = find_block_lines(&lines, path) else {
        return content.to_string();
    };

    if let Some(existing_idx) = find_property_line(&lines, &range, property) {
        lines.remove(existing_idx);
    }

    let mut updated = lines.join("\n");
    if content.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

fn upsert_property(content: &str, path: &[&str], property: &str, value: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
    let range = ensure_block_path(&mut lines, path);
    let replacement = format!("{}{} {}", " ".repeat(range.indent + 4), property, value);

    if let Some(existing_idx) = find_property_line(&lines, &range, property) {
        lines[existing_idx] = replacement;
    } else {
        lines.insert(range.end, replacement);
    }

    let mut updated = lines.join("\n");
    if content.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

#[derive(Clone, Copy)]
struct BlockRange {
    start: usize,
    end: usize,
    indent: usize,
}

fn ensure_block_path(lines: &mut Vec<String>, path: &[&str]) -> BlockRange {
    let mut prefix: Vec<&str> = Vec::new();

    for segment in path {
        prefix.push(segment);
        if find_block_lines(lines, &prefix).is_some() {
            continue;
        }

        let parent = if prefix.len() > 1 {
            find_block_lines(lines, &prefix[..prefix.len() - 1]).expect("parent block must exist")
        } else {
            BlockRange {
                start: 0,
                end: lines.len(),
                indent: 0,
            }
        };

        let indent = if prefix.len() > 1 {
            parent.indent + 4
        } else {
            0
        };
        let open = format!("{}{} {{", " ".repeat(indent), segment);
        let close = format!("{}}}", " ".repeat(indent));
        lines.insert(parent.end, open);
        lines.insert(parent.end + 1, close);
    }

    find_block_lines(lines, path).expect("block path should exist after creation")
}

fn find_property_line(lines: &[String], block: &BlockRange, property: &str) -> Option<usize> {
    (block.start + 1..block.end).find(|idx| {
        let trimmed = strip_comment(&lines[*idx]).trim();
        trimmed
            .strip_prefix(property)
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    })
}

fn find_block_lines(lines: &[impl AsRef<str>], path: &[&str]) -> Option<BlockRange> {
    let mut search_start = 0usize;
    let mut search_end = lines.len();
    let mut current = None;

    for segment in path {
        let next = find_named_block(lines, search_start, search_end, segment)?;
        search_start = next.start + 1;
        search_end = next.end;
        current = Some(next);
    }

    current
}

fn find_named_block(
    lines: &[impl AsRef<str>],
    search_start: usize,
    search_end: usize,
    name: &str,
) -> Option<BlockRange> {
    let mut depth = 0usize;

    for idx in search_start..search_end {
        let line = lines[idx].as_ref();
        let code = strip_comment(line);
        let trimmed = code.trim();

        if depth == 0 && line_indent(line) == 0 && trimmed.starts_with(&format!("{name} {{")) {
            let end = find_block_end(lines, idx)?;
            return Some(BlockRange {
                start: idx,
                end,
                indent: line_indent(line),
            });
        }

        if depth == 0 && trimmed.starts_with(name) && trimmed.ends_with('{') {
            let end = find_block_end(lines, idx)?;
            return Some(BlockRange {
                start: idx,
                end,
                indent: line_indent(line),
            });
        }

        depth = update_depth(depth, &code);
    }

    None
}

fn find_block_end(lines: &[impl AsRef<str>], start: usize) -> Option<usize> {
    let mut depth = 0usize;

    for (idx, line) in lines.iter().enumerate().skip(start) {
        depth = update_depth(depth, strip_comment(line.as_ref()));
        if depth == 0 {
            return Some(idx);
        }
    }

    None
}

fn update_depth(mut depth: usize, line: &str) -> usize {
    let opens = line.matches('{').count();
    let closes = line.matches('}').count();
    depth += opens;
    depth = depth.saturating_sub(closes);
    depth
}

fn strip_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(code, _)| code)
}

fn line_indent(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create directory {}", parent.display()))
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn localectl_x11_layouts() -> Option<Vec<String>> {
    let output = Command::new("localectl").arg("status").output().ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("X11 Layout:") {
            let layouts = split_csv(rest);
            if !layouts.is_empty() {
                return Some(layouts);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        NiriWorkspace, find_property_value, quoted, remove_property, scratchpad_name, split_csv,
        upsert_property, workspace_reference,
    };

    #[test]
    fn updates_existing_niri_property() {
        let source = r#"input {
    mouse {
        accel-speed 0.2
    }
}
"#;

        let updated = upsert_property(source, &["input", "mouse"], "accel-speed", "0.4");
        assert!(updated.contains("accel-speed 0.4"));
    }

    #[test]
    fn creates_missing_nested_niri_blocks() {
        let source = "layout {\n    gaps 16\n}\n";
        let updated = upsert_property(source, &["input", "keyboard", "xkb"], "layout", "\"de,us\"");

        assert!(updated.contains("input {"));
        assert!(updated.contains("keyboard {"));
        assert!(updated.contains("xkb {"));
        assert!(updated.contains("layout \"de,us\""));
    }

    #[test]
    fn reads_existing_niri_property() {
        let source = r#"input {
    touchpad {
        accel-speed -0.5
    }
}
"#;

        let value = find_property_value(source, &["input", "touchpad"], "accel-speed");
        assert_eq!(value.as_deref(), Some("-0.5"));
    }

    #[test]
    fn removes_conflicting_niri_property() {
        let source = r#"input {
    keyboard {
        xkb {
            file "~/.config/keymap.xkb"
            layout "us"
        }
    }
}
"#;

        let updated = remove_property(source, &["input", "keyboard", "xkb"], "file");
        assert!(!updated.contains("file \"~/.config/keymap.xkb\""));
        assert!(updated.contains("layout \"us\""));
    }

    #[test]
    fn splits_csv_layouts() {
        assert_eq!(split_csv("us, de ,ru"), vec!["us", "de", "ru"]);
    }

    #[test]
    fn quotes_niri_string_properties() {
        assert_eq!(quoted("flat"), "\"flat\"");
    }

    #[test]
    fn parses_scratchpad_name_from_app_id() {
        assert_eq!(scratchpad_name(Some("scratchpad_menu")), Some("menu"));
        assert_eq!(scratchpad_name(Some("kitty")), None);
        assert_eq!(scratchpad_name(None), None);
    }

    #[test]
    fn prefers_workspace_name_over_index() {
        let named = NiriWorkspace {
            id: 1,
            idx: 3,
            name: Some("scratchpad_test".to_string()),
            is_focused: false,
            active_window_id: None,
        };
        assert_eq!(workspace_reference(&named), "scratchpad_test");

        let unnamed = NiriWorkspace {
            id: 2,
            idx: 4,
            name: None,
            is_focused: false,
            active_window_id: None,
        };
        assert_eq!(workspace_reference(&unnamed), "4");
    }
}
