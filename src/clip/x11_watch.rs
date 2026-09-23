//! X11 clipboard watcher feeding `cliphist`.
//!
//! This is the X11 counterpart of `wl-paste --watch cliphist store`. XFixes
//! delivers an event whenever the CLIPBOARD owner changes; the data itself is
//! read with `xclip`, which already implements the full ICCCM transfer
//! protocol (including INCR for large images). Using cliphist on both display
//! servers keeps a single history format for the menu, previews, and restore.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use x11rb::NONE;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};

/// Seconds the selection owner may take to hand over a target. Keep this
/// generous: killing a reader in the middle of an INCR transfer leaves owners
/// such as `xclip -i` waiting forever, which wedges the clipboard (this is how
/// clipmenud's `timeout 1 xsel` broke large screenshots).
const OWNER_TIMEOUT_SECS: &str = "10";
/// Upper bound for a single clipboard entry; larger payloads are skipped.
const MAX_ENTRY_BYTES: usize = 64 * 1024 * 1024;
/// Target advertised by password managers (KeePassXC, KDE) for secrets.
const PASSWORD_MANAGER_HINT: &str = "x-kde-passwordManagerHint";

/// Preferred targets, best first. Text wins over images like in `wl-paste`,
/// so e.g. office suites offering both a bitmap and text store the text.
const TARGET_PREFERENCE: &[&str] = &[
    "UTF8_STRING",
    "text/plain;charset=utf-8",
    "STRING",
    "text/plain",
    "TEXT",
    "image/png",
    "image/webp",
    "image/jpeg",
    "image/gif",
    "image/bmp",
];

pub fn run() -> Result<()> {
    if live_wayland_session() {
        eprintln!("A Wayland session is active; cliphist.service handles capture there. Exiting.");
        return Ok(());
    }

    let (conn, screen_num) = x11rb::connect(None)
        .context("Failed to connect to the X server (is DISPLAY set for the user session?)")?;
    let screen = &conn.setup().roots[screen_num];
    let window = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )?;
    let clipboard = conn.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
    conn.xfixes_query_version(5, 0)?
        .reply()
        .context("The X server does not support the XFixes extension")?;
    conn.xfixes_select_selection_input(window, clipboard, SelectionEventMask::SET_SELECTION_OWNER)?;
    conn.flush()?;
    eprintln!("Watching the X11 CLIPBOARD selection for cliphist");

    // Like `wl-paste --watch`, record whatever is already on the clipboard.
    if conn.get_selection_owner(clipboard)?.reply()?.owner != NONE {
        capture_logged();
    }

    loop {
        let Event::XfixesSelectionNotify(mut latest) = conn.wait_for_event()? else {
            continue;
        };
        // Collapse bursts of owner changes into the most recent one.
        while let Some(event) = conn.poll_for_event()? {
            if let Event::XfixesSelectionNotify(notify) = event {
                latest = notify;
            }
        }
        if latest.owner != NONE {
            capture_logged();
        }
    }
}

/// True when `WAYLAND_DISPLAY` points at a compositor that actually accepts
/// connections. The systemd user environment often keeps a stale value from
/// an earlier session, so the variable alone is not trustworthy.
fn live_wayland_session() -> bool {
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY").filter(|value| !value.is_empty())
    else {
        return false;
    };
    let path = std::path::PathBuf::from(display);
    let socket = if path.is_absolute() {
        path
    } else {
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(runtime) => std::path::PathBuf::from(runtime).join(path),
            None => return false,
        }
    };
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}

fn capture_logged() {
    if let Err(error) = capture() {
        eprintln!("Skipping clipboard change: {error:#}");
    }
}

fn capture() -> Result<()> {
    // Owners that do not answer TARGETS still get a plain text attempt.
    let targets = read_target("TARGETS")
        .map(|data| parse_targets(&data))
        .unwrap_or_default();
    if targets.iter().any(|target| target == PASSWORD_MANAGER_HINT) {
        return Ok(());
    }

    for target in choose_targets(&targets) {
        let data = match read_target(target) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("Could not read {target}: {error:#}");
                continue;
            }
        };
        if let Some(data) = normalize(data, target) {
            return store_in_cliphist(&data);
        }
    }
    Ok(())
}

fn parse_targets(data: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(data)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn choose_targets(offered: &[String]) -> Vec<&'static str> {
    if offered.is_empty() {
        return vec!["UTF8_STRING", "STRING"];
    }
    TARGET_PREFERENCE
        .iter()
        .copied()
        .filter(|target| offered.iter().any(|offer| offer == target))
        .collect()
}

fn read_target(target: &str) -> Result<Vec<u8>> {
    let output = Command::new("timeout")
        .args([OWNER_TIMEOUT_SECS, "xclip", "-selection", "clipboard", "-o"])
        .args(["-t", target])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("Failed to run xclip")?;
    anyhow::ensure!(
        output.status.success(),
        "xclip could not read target {target} ({})",
        output.status
    );
    Ok(output.stdout)
}

/// Convert selection data into what cliphist should store, or `None` when
/// the entry is not worth recording.
fn normalize(data: Vec<u8>, target: &str) -> Option<Vec<u8>> {
    if data.is_empty() || data.len() > MAX_ENTRY_BYTES {
        return None;
    }
    if target.starts_with("image/") {
        return Some(data);
    }
    // STRING is ISO-8859-1 per ICCCM; store UTF-8 so previews work.
    let data = if target == "STRING" && std::str::from_utf8(&data).is_err() {
        data.iter()
            .map(|&byte| byte as char)
            .collect::<String>()
            .into_bytes()
    } else {
        data
    };
    data.iter()
        .any(|byte| !byte.is_ascii_whitespace())
        .then_some(data)
}

fn store_in_cliphist(data: &[u8]) -> Result<()> {
    let mut child = Command::new("cliphist")
        .arg("store")
        // Only meaningful for wl-paste; never let a stray value drop entries.
        .env_remove("CLIPBOARD_STATE")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to run cliphist store")?;
    {
        let mut stdin = child
            .stdin
            .take()
            .context("Failed to open cliphist input")?;
        stdin
            .write_all(data)
            .context("Failed to send clipboard data to cliphist")?;
    }
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "cliphist store failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(targets: &[&str]) -> Vec<String> {
        targets.iter().map(|target| target.to_string()).collect()
    }

    #[test]
    fn prefers_text_over_images() {
        let targets = offered(&["TARGETS", "image/png", "UTF8_STRING", "text/html"]);
        assert_eq!(choose_targets(&targets), vec!["UTF8_STRING", "image/png"]);
    }

    #[test]
    fn picks_images_when_no_text_is_offered() {
        let targets = offered(&["TARGETS", "TIMESTAMP", "image/png"]);
        assert_eq!(choose_targets(&targets), vec!["image/png"]);
    }

    #[test]
    fn falls_back_to_text_without_targets() {
        assert_eq!(choose_targets(&[]), vec!["UTF8_STRING", "STRING"]);
    }

    #[test]
    fn parses_xclip_targets_output() {
        assert_eq!(
            parse_targets(b"TARGETS\nimage/png\n\n"),
            offered(&["TARGETS", "image/png"])
        );
    }

    #[test]
    fn whitespace_only_text_is_skipped() {
        assert_eq!(normalize(b"  \n\t".to_vec(), "UTF8_STRING"), None);
        assert_eq!(normalize(Vec::new(), "image/png"), None);
        assert_eq!(
            normalize(b" hi ".to_vec(), "UTF8_STRING"),
            Some(b" hi ".to_vec())
        );
    }

    #[test]
    fn latin1_string_is_converted_to_utf8() {
        assert_eq!(
            normalize(vec![b'c', b'a', b'f', 0xe9], "STRING"),
            Some("café".as_bytes().to_vec())
        );
        assert_eq!(
            normalize("café".as_bytes().to_vec(), "STRING"),
            Some("café".as_bytes().to_vec())
        );
    }
}
