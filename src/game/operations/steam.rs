use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

#[derive(Debug, Clone)]
struct SteamShortcut {
    app_name: String,
    exe: String,
    start_dir: String,
    icon: String,
    shortcut_path: String,
    launch_options: String,
    is_hidden: u32,
    allow_desktop_config: u32,
    allow_overlay: u32,
    openvr: u32,
    devkit: u32,
    devkit_game_id: String,
    last_play_time: u32,
    tags: Vec<String>,
}

impl SteamShortcut {
    fn new(app_name: &str, exe: &str, start_dir: &str, launch_options: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            exe: exe.to_string(),
            start_dir: start_dir.to_string(),
            icon: String::new(),
            shortcut_path: String::new(),
            launch_options: launch_options.to_string(),
            is_hidden: 0,
            allow_desktop_config: 1,
            allow_overlay: 1,
            openvr: 0,
            devkit: 0,
            devkit_game_id: String::new(),
            last_play_time: 0,
            tags: Vec::new(),
        }
    }
}

fn find_steam_userdata_dirs() -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;

    let candidates = [
        home.join(".local/share/Steam/userdata"),
        home.join(".steam/steam/userdata"),
    ];

    for base in &candidates {
        if base.is_dir()
            && let Ok(entries) = std::fs::read_dir(base)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && name.chars().all(|c| c.is_ascii_digit())
                {
                    dirs.push(path);
                }
            }
        }
    }

    dirs.sort();
    dirs.dedup_by(|a, b| match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    });

    Ok(dirs)
}

fn shortcuts_vdf_path(userdata_dir: &Path) -> PathBuf {
    userdata_dir.join("config").join("shortcuts.vdf")
}

fn read_shortcuts_vdf(path: &Path) -> Result<Vec<SteamShortcut>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read(path).context("Failed to read shortcuts.vdf")?;
    parse_shortcuts_vdf(&data)
}

fn parse_shortcuts_vdf(data: &[u8]) -> Result<Vec<SteamShortcut>> {
    let mut pos = 0;
    let mut shortcuts = Vec::new();

    if data.is_empty() {
        return Ok(shortcuts);
    }

    if pos >= data.len() || data[pos] != 0x00 {
        return Ok(shortcuts);
    }
    pos += 1;

    let name_end = data[pos..]
        .iter()
        .position(|&b| b == 0x00)
        .ok_or_else(|| anyhow!("Invalid VDF: unterminated string"))?;
    pos += name_end + 1;

    loop {
        if pos >= data.len() || data[pos] == 0x08 {
            break;
        }

        if data[pos] != 0x00 {
            break;
        }
        pos += 1;

        let name_end = data[pos..]
            .iter()
            .position(|&b| b == 0x00)
            .ok_or_else(|| anyhow!("Invalid VDF: unterminated entry name"))?;
        pos += name_end + 1;

        let mut shortcut = SteamShortcut::new("", "", "", "");
        let mut tags = Vec::new();

        loop {
            if pos >= data.len() || data[pos] == 0x08 {
                pos += 1;
                break;
            }

            let field_type = data[pos];
            pos += 1;

            let name_end = data[pos..]
                .iter()
                .position(|&b| b == 0x00)
                .ok_or_else(|| anyhow!("Invalid VDF: unterminated field name"))?;
            let field_name = String::from_utf8_lossy(&data[pos..pos + name_end]).to_string();
            pos += name_end + 1;

            match field_type {
                0x01 => {
                    let val_end = data[pos..]
                        .iter()
                        .position(|&b| b == 0x00)
                        .ok_or_else(|| anyhow!("Invalid VDF: unterminated string value"))?;
                    let value = String::from_utf8_lossy(&data[pos..pos + val_end]).to_string();
                    pos += val_end + 1;

                    match field_name.to_lowercase().as_str() {
                        "appname" => shortcut.app_name = value,
                        "exe" => shortcut.exe = value,
                        "startdir" => shortcut.start_dir = value,
                        "icon" => shortcut.icon = value,
                        "shortcutpath" => shortcut.shortcut_path = value,
                        "launchoptions" => shortcut.launch_options = value,
                        "devkitgameid" => shortcut.devkit_game_id = value,
                        _ => {}
                    }
                }
                0x02 => {
                    if pos + 4 > data.len() {
                        bail!("Invalid VDF: truncated integer");
                    }
                    let value = u32::from_le_bytes([
                        data[pos],
                        data[pos + 1],
                        data[pos + 2],
                        data[pos + 3],
                    ]);
                    pos += 4;

                    match field_name.to_lowercase().as_str() {
                        "ishidden" => shortcut.is_hidden = value,
                        "allowdesktopconfig" => shortcut.allow_desktop_config = value,
                        "allowoverlay" => shortcut.allow_overlay = value,
                        "openvr" => shortcut.openvr = value,
                        "devkit" => shortcut.devkit = value,
                        "lastplaytime" => shortcut.last_play_time = value,
                        _ => {}
                    }
                }
                0x00 => {
                    if field_name.to_lowercase() == "tags" {
                        loop {
                            if pos >= data.len() || data[pos] == 0x08 {
                                pos += 1;
                                break;
                            }
                            let tag_type = data[pos];
                            pos += 1;

                            let idx_end = data[pos..]
                                .iter()
                                .position(|&b| b == 0x00)
                                .ok_or_else(|| anyhow!("Invalid VDF: unterminated tag index"))?;
                            pos += idx_end + 1;

                            if tag_type == 0x01 {
                                let val_end =
                                    data[pos..].iter().position(|&b| b == 0x00).ok_or_else(
                                        || anyhow!("Invalid VDF: unterminated tag value"),
                                    )?;
                                let tag_val =
                                    String::from_utf8_lossy(&data[pos..pos + val_end]).to_string();
                                pos += val_end + 1;
                                tags.push(tag_val);
                            }
                        }
                    } else {
                        skip_map(data, &mut pos)?;
                    }
                }
                _ => {
                    break;
                }
            }
        }

        shortcut.tags = tags;
        shortcuts.push(shortcut);
    }

    Ok(shortcuts)
}

fn skip_map(data: &[u8], pos: &mut usize) -> Result<()> {
    loop {
        if *pos >= data.len() || data[*pos] == 0x08 {
            *pos += 1;
            break;
        }
        let field_type = data[*pos];
        *pos += 1;

        let name_end = data[*pos..]
            .iter()
            .position(|&b| b == 0x00)
            .ok_or_else(|| anyhow!("Invalid VDF: unterminated field name in skip"))?;
        *pos += name_end + 1;

        match field_type {
            0x01 => {
                let val_end = data[*pos..]
                    .iter()
                    .position(|&b| b == 0x00)
                    .ok_or_else(|| anyhow!("Invalid VDF: unterminated string in skip"))?;
                *pos += val_end + 1;
            }
            0x02 => {
                *pos += 4;
            }
            0x00 => {
                skip_map(data, pos)?;
            }
            _ => break,
        }
    }
    Ok(())
}

fn write_shortcuts_vdf(shortcuts: &[SteamShortcut]) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.push(0x00);
    buf.extend_from_slice(b"shortcuts\0");

    for (i, shortcut) in shortcuts.iter().enumerate() {
        buf.push(0x00);
        buf.extend_from_slice(i.to_string().as_bytes());
        buf.push(0x00);

        write_vdf_string(&mut buf, "AppName", &shortcut.app_name);
        write_vdf_string(&mut buf, "exe", &shortcut.exe);
        write_vdf_string(&mut buf, "StartDir", &shortcut.start_dir);
        write_vdf_string(&mut buf, "icon", &shortcut.icon);
        write_vdf_string(&mut buf, "ShortcutPath", &shortcut.shortcut_path);
        write_vdf_string(&mut buf, "LaunchOptions", &shortcut.launch_options);

        write_vdf_int(&mut buf, "IsHidden", shortcut.is_hidden);
        write_vdf_int(
            &mut buf,
            "AllowDesktopConfig",
            shortcut.allow_desktop_config,
        );
        write_vdf_int(&mut buf, "AllowOverlay", shortcut.allow_overlay);
        write_vdf_int(&mut buf, "OpenVR", shortcut.openvr);
        write_vdf_int(&mut buf, "Devkit", shortcut.devkit);
        write_vdf_string(&mut buf, "DevkitGameID", &shortcut.devkit_game_id);
        write_vdf_int(&mut buf, "LastPlayTime", shortcut.last_play_time);

        buf.push(0x00);
        buf.extend_from_slice(b"tags\0");
        for (j, tag) in shortcut.tags.iter().enumerate() {
            write_vdf_string(&mut buf, &j.to_string(), tag);
        }
        buf.push(0x08);

        buf.push(0x08);
    }

    buf.push(0x08);
    buf.push(0x08);

    buf
}

fn write_vdf_string(buf: &mut Vec<u8>, name: &str, value: &str) {
    buf.push(0x01);
    buf.extend_from_slice(name.as_bytes());
    buf.push(0x00);
    buf.extend_from_slice(value.as_bytes());
    buf.push(0x00);
}

fn write_vdf_int(buf: &mut Vec<u8>, name: &str, value: u32) {
    buf.push(0x02);
    buf.extend_from_slice(name.as_bytes());
    buf.push(0x00);
    buf.extend_from_slice(&value.to_le_bytes());
}

fn is_steam_running() -> bool {
    std::process::Command::new("pgrep")
        .arg("-x")
        .arg("steam")
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn add_game_to_steam(game_name: &str, _launch_command: &str) -> Result<bool> {
    if is_steam_running() {
        bail!(
            "Steam is currently running. Please close Steam before adding shortcuts.\n\
             Steam overwrites shortcuts.vdf while running."
        );
    }

    let userdata_dirs = find_steam_userdata_dirs()?;
    if userdata_dirs.is_empty() {
        bail!(
            "No Steam userdata directories found.\n\
             Is Steam installed?"
        );
    }

    let ins_bin = std::env::current_exe().context("Failed to determine ins binary path")?;
    let ins_bin_str = ins_bin.to_string_lossy().to_string();

    let exe = format!("\"{}\"", ins_bin_str);
    let start_dir = format!(
        "\"{}\"",
        ins_bin
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    let launch_options = format!("game launch \"{}\"", game_name);

    let mut added_count = 0;

    for userdata_dir in &userdata_dirs {
        let vdf_path = shortcuts_vdf_path(userdata_dir);

        if let Some(parent) = vdf_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let mut shortcuts = read_shortcuts_vdf(&vdf_path).unwrap_or_default();

        let already_exists = shortcuts.iter().any(|s| s.app_name == game_name);
        if already_exists {
            continue;
        }

        let mut shortcut = SteamShortcut::new(game_name, &exe, &start_dir, &launch_options);
        shortcut.tags = vec!["ins-game".to_string()];
        shortcuts.push(shortcut);

        let data = write_shortcuts_vdf(&shortcuts);

        if vdf_path.exists() {
            let backup_path = vdf_path.with_extension("vdf.bak");
            std::fs::copy(&vdf_path, &backup_path).context("Failed to backup shortcuts.vdf")?;
        }

        std::fs::write(&vdf_path, data).context("Failed to write shortcuts.vdf")?;

        added_count += 1;
    }

    Ok(added_count > 0)
}

pub fn add_game_menu_to_steam() -> Result<bool> {
    use crate::common::terminal::detect_terminal;

    if is_steam_running() {
        bail!(
            "Steam is currently running. Please close Steam before adding shortcuts.\n\
             Steam overwrites shortcuts.vdf while running."
        );
    }

    let userdata_dirs = find_steam_userdata_dirs()?;
    if userdata_dirs.is_empty() {
        bail!(
            "No Steam userdata directories found.\n\
             Is Steam installed?"
        );
    }

    let ins_bin = std::env::current_exe().context("Failed to determine ins binary path")?;
    let ins_bin_str = ins_bin.to_string_lossy().to_string();

    let terminal = detect_terminal();
    let terminal_path = which::which(&terminal).context("Failed to find terminal emulator")?;
    let terminal_str = terminal_path.to_string_lossy().to_string();

    let exe = format!("\"{}\"", terminal_str);
    let start_dir = format!(
        "\"{}\"",
        ins_bin
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    let launch_options = format!("-- \"{}\" game menu", ins_bin_str);

    let app_name = "ins game menu";

    let mut added_count = 0;

    for userdata_dir in &userdata_dirs {
        let vdf_path = shortcuts_vdf_path(userdata_dir);

        if let Some(parent) = vdf_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let mut shortcuts = read_shortcuts_vdf(&vdf_path).unwrap_or_default();

        let already_exists = shortcuts.iter().any(|s| s.app_name == app_name);
        if already_exists {
            continue;
        }

        let mut shortcut = SteamShortcut::new(app_name, &exe, &start_dir, &launch_options);
        shortcut.tags = vec!["ins-game".to_string()];
        shortcuts.push(shortcut);

        let data = write_shortcuts_vdf(&shortcuts);

        if vdf_path.exists() {
            let backup_path = vdf_path.with_extension("vdf.bak");
            std::fs::copy(&vdf_path, &backup_path).context("Failed to backup shortcuts.vdf")?;
        }

        std::fs::write(&vdf_path, data).context("Failed to write shortcuts.vdf")?;

        added_count += 1;
    }

    Ok(added_count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_empty() {
        let data = write_shortcuts_vdf(&[]);
        let parsed = parse_shortcuts_vdf(&data).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn roundtrip_single_shortcut() {
        let mut shortcut =
            SteamShortcut::new("TestGame", "\"/usr/bin/test\"", "\"/usr/bin\"", "--foo");
        shortcut.tags = vec!["tag1".to_string(), "tag2".to_string()];
        shortcut.is_hidden = 1;
        shortcut.allow_overlay = 0;

        let data = write_shortcuts_vdf(&[shortcut]);
        let parsed = parse_shortcuts_vdf(&data).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].app_name, "TestGame");
        assert_eq!(parsed[0].exe, "\"/usr/bin/test\"");
        assert_eq!(parsed[0].start_dir, "\"/usr/bin\"");
        assert_eq!(parsed[0].launch_options, "--foo");
        assert_eq!(parsed[0].is_hidden, 1);
        assert_eq!(parsed[0].allow_overlay, 0);
        assert_eq!(parsed[0].allow_desktop_config, 1);
        assert_eq!(parsed[0].tags, vec!["tag1", "tag2"]);
    }

    #[test]
    fn roundtrip_multiple_shortcuts() {
        let shortcuts = vec![
            SteamShortcut::new("Game1", "/bin/g1", "/home", ""),
            SteamShortcut::new("Game2", "/bin/g2", "/home", "--opt"),
        ];

        let data = write_shortcuts_vdf(&shortcuts);
        let parsed = parse_shortcuts_vdf(&data).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].app_name, "Game1");
        assert_eq!(parsed[1].app_name, "Game2");
        assert_eq!(parsed[1].launch_options, "--opt");
    }

    #[test]
    fn parse_empty_data() {
        let parsed = parse_shortcuts_vdf(&[]).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn read_nonexistent_file() {
        let path = Path::new("/tmp/instantcli-test-nonexistent-shortcuts.vdf");
        let result = read_shortcuts_vdf(path).unwrap();
        assert!(result.is_empty());
    }
}
