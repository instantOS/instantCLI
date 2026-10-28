use super::ScratchpadProvider;
use super::ScratchpadWindowInfo;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result, bail};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use std::io::{ErrorKind, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::Builder;

pub struct KWin;

#[derive(serde::Deserialize, Debug)]
struct KWinWindowInfo {
    name: String,
    class: String,
    title: String,
    visible: bool,
    #[allow(dead_code)]
    pid: Option<u32>,
}

impl ScratchpadProvider for KWin {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        // Try to show window first (fast if exists)
        self.run_script(config, "show")?;

        // Check if window exists - if not, create it
        // We do this check AFTER trying to show to optimize for the common case
        if !self.is_window_running(config)? {
            config.launcher().spawn_detached()?;
            std::thread::sleep(std::time::Duration::from_millis(700));
            self.run_script(config, "show")?;
        }
        Ok(())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        // Hide doesn't need to check if window exists
        self.run_script(config, "hide")
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        // Try to toggle window first (fast if exists)
        self.run_script(config, "toggle")?;

        // Check if window exists - if not, create it
        // We do this check AFTER trying to toggle to optimize for the common case
        if !self.is_window_running(config)? {
            config.launcher().spawn_detached()?;
            std::thread::sleep(std::time::Duration::from_millis(700));
            self.run_script(config, "show")?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        Ok(self
            .get_kwin_window_info()?
            .into_iter()
            .map(|window| ScratchpadWindowInfo {
                name: window.name,
                window_class: window.class,
                title: window.title,
                visible: window.visible,
            })
            .collect())
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        let class = config.window_class();
        Ok(self
            .get_kwin_window_info()?
            .iter()
            .any(|window| window.class == class))
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        let class = config.window_class();
        Ok(self
            .get_kwin_window_info()?
            .iter()
            .any(|window| window.class == class && window.visible))
    }

    fn supports_scratchpad(&self) -> bool {
        // KWin only supports scratchpad on Wayland
        use crate::common::display_server::DisplayServer;
        DisplayServer::detect() == DisplayServer::Wayland
    }
}

impl KWin {
    /// Get window info from KWin via scripting and DBus monitoring
    fn get_kwin_window_info(&self) -> Result<Vec<KWinWindowInfo>> {
        // Start dbus-monitor to capture the callDBus output
        // We use a fake destination that the script will call
        let monitor_dest = "org.instantos.scratchpad.status";
        let mut monitor = Command::new("dbus-monitor")
            .args(["--session", &format!("destination='{}'", monitor_dest)])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to start dbus-monitor; install the D-Bus tools required for KWin scratchpads")?;

        // Give dbus-monitor a moment to start
        std::thread::sleep(Duration::from_millis(10));

        // Prepare script that calls DBus
        // callDBus(service, path, interface, method, arg...)
        let script_content = format!(
            r#"
            (function() {{
                var clients = workspace.windowList();
                var res = [];
                for (var i = 0; i < clients.length; i++) {{
                    var c = clients[i];
                    if (c.resourceClass.indexOf("scratchpad_") === 0 || c.resourceClass === "instantscratchpad") {{
                        var visible = !c.minimized && (c.desktops.indexOf(workspace.currentDesktop) !== -1 || c.onAllDesktops);
                        res.push({{
                            name: c.resourceClass.replace("scratchpad_", ""),
                            class: c.resourceClass,
                            title: c.caption,
                            visible: visible,
                            pid: c.pid
                        }});
                    }}
                }}
                callDBus("{}", "/Data", "org.instantos.Scratchpad", "notify", JSON.stringify(res));
            }})();
            "#,
            monitor_dest
        );

        let result = (|| {
            self.execute_kwin_script(&script_content)?;
            let mut stdout = monitor
                .stdout
                .take()
                .context("Missing KWin monitor output")?;
            let flags = OFlag::from_bits_truncate(fcntl(&stdout, FcntlArg::F_GETFL)?);
            fcntl(&stdout, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
            read_window_info(&mut stdout, Duration::from_millis(500))
        })();

        // Reap the monitor on success and on every error path.
        let _ = monitor.kill();
        let _ = monitor.wait();
        result
    }

    /// Helper to execute a raw KWin script string
    fn execute_kwin_script(&self, script_content: &str) -> Result<()> {
        let mut temp_file = Builder::new()
            .prefix("kwin-query-")
            .suffix(".js")
            .tempfile()?;

        write!(temp_file, "{}", script_content)?;
        temp_file.flush()?;
        let path = temp_file.path().to_str().context("Invalid path")?;

        let output = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=500",
                "--dest=org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                &format!("string:{}", path),
            ])
            .output()
            .context("Failed to execute dbus-send to load KWin script")?;

        anyhow::ensure!(
            output.status.success(),
            "Failed to load KWin script: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        let id_str = stdout.lines().last().unwrap_or("").trim();
        let id_parts: Vec<&str> = id_str.split_whitespace().collect();
        let id = if id_parts.len() >= 2 && id_parts[0] == "int32" {
            id_parts[1]
        } else {
            return Err(anyhow::anyhow!("Invalid script ID"));
        };

        let script_obj_path = format!("/Scripting/Script{}", id);
        let run_result = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=500",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.run",
            ])
            .output()
            .context("Failed to run KWin script");

        // Stop it immediately
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.stop",
            ])
            .output();

        let output = run_result?;
        anyhow::ensure!(
            output.status.success(),
            "Failed to run KWin script: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    fn run_script(&self, config: &ScratchpadConfig, action: &str) -> Result<()> {
        let class = config.window_class();
        let width_pct = config.width_pct as f64 / 100.0;
        let height_pct = config.height_pct as f64 / 100.0;

        // Plasma 6 Script - Enhanced with geometry and positioning
        let script_content = format!(
            r#"
            (function() {{
                const clients = workspace.windowList();
                const targetClass = "{class}";
                const action = "{action}";
                const widthPct = {width_pct};
                const heightPct = {height_pct};

                for (let i = 0; i < clients.length; i++) {{
                    const client = clients[i];
                    if (client.resourceClass === targetClass) {{
                        // Configure window properties for scratchpad behavior
                        client.keepAbove = true;
                        client.skipTaskbar = true;
                        client.skipSwitcher = true;
                        client.skipPager = true;

                        // Set window geometry (centered, sized to percentages)
                        const maxBounds = workspace.clientArea(
                            KWin.PlacementArea,
                            workspace.activeScreen,
                            workspace.currentDesktop
                        );
                        const targetWidth = Math.round(maxBounds.width * widthPct);
                        const targetHeight = Math.round(maxBounds.height * heightPct);
                        const targetX = maxBounds.x + Math.round((maxBounds.width - targetWidth) / 2);
                        const targetY = maxBounds.y + Math.round((maxBounds.height - targetHeight) / 2);
                        
                        client.frameGeometry = {{
                            x: targetX,
                            y: targetY,
                            width: targetWidth,
                            height: targetHeight
                        }};

                        if (action === "toggle") {{
                            if (workspace.activeWindow === client && !client.minimized) {{
                                client.minimized = true;
                            }} else {{
                                client.minimized = false;
                                client.desktops = [workspace.currentDesktop];
                                workspace.activeWindow = client;
                            }}
                        }} else if (action === "show") {{
                            client.minimized = false;
                            client.desktops = [workspace.currentDesktop];
                            workspace.activeWindow = client;
                        }} else if (action === "hide") {{
                            client.minimized = true;
                        }}
                        
                        print("WINDOW_FOUND");
                        return;
                    }}
                }}
                print("WINDOW_NOT_FOUND");
            }})();
        "#
        );

        self.execute_kwin_script(&script_content)
    }
}

/// Read a nonblocking monitor pipe so a quiet bus cannot bypass the deadline.
fn read_window_info(reader: &mut impl Read, timeout: Duration) -> Result<Vec<KWinWindowInfo>> {
    let deadline = Instant::now() + timeout;
    let mut pending = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("Timeout waiting for KWin status");
        }
        match reader.read(&mut buffer) {
            Ok(0) => {
                if let Some(windows) = parse_window_info(&pending)? {
                    return Ok(windows);
                }
                bail!("KWin status monitor exited without a status response");
            }
            Ok(count) => {
                pending.extend_from_slice(&buffer[..count]);
                while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                    if let Some(windows) = parse_window_info(&pending[..end])? {
                        return Ok(windows);
                    }
                    pending.drain(..=end);
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("Failed to read KWin status"),
        }
    }
}

fn parse_window_info(line: &[u8]) -> Result<Option<Vec<KWinWindowInfo>>> {
    let line = std::str::from_utf8(line)
        .context("Invalid UTF-8 in KWin status")?
        .trim();
    let Some(quoted) = line.strip_prefix("string ") else {
        return Ok(None);
    };
    let Some(contents) = quoted
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return Ok(None);
    };
    if !contents.starts_with('[') {
        return Ok(None);
    }
    // Accept verbatim JSON and escaped strings. Decode the complete string
    // instead of replacing quotes, preserving escapes
    // in window titles (backslashes, quotes, newlines and Unicode).
    let windows = match serde_json::from_str(contents) {
        Ok(windows) => windows,
        Err(_) => {
            let decoded: String =
                serde_json::from_str(quoted).context("Failed to decode KWin status string")?;
            serde_json::from_str(&decoded).context("Failed to parse KWin window info JSON")?
        }
    };
    Ok(Some(windows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::net::UnixStream;

    #[test]
    fn empty_window_list_is_a_successful_status_response() -> Result<()> {
        let mut output = Cursor::new(b"method call ...\n   string \"[]\"\n");
        assert!(read_window_info(&mut output, Duration::from_secs(1))?.is_empty());
        Ok(())
    }

    #[test]
    fn status_preserves_titles_in_raw_and_escaped_monitor_output() -> Result<()> {
        let title = "Notes \"quoted\" at C:\\saves\n日本語";
        let payload = serde_json::to_string(&serde_json::json!([{
            "name": "notes", "class": "scratchpad_notes", "title": title, "visible": true
        }]))?;
        for quoted in [format!("\"{payload}\""), serde_json::to_string(&payload)?] {
            let mut output = Cursor::new(format!("method call ...\n   string {quoted}\n"));
            let windows = read_window_info(&mut output, Duration::from_secs(1))?;
            assert_eq!(windows.len(), 1);
            assert_eq!(windows[0].title, title);
            assert!(windows[0].visible);
        }
        Ok(())
    }

    #[test]
    fn silent_or_partial_monitor_output_cannot_block_past_the_deadline() -> Result<()> {
        for partial in [b"".as_slice(), b"   string \"[".as_slice()] {
            let (mut reader, mut writer) = UnixStream::pair()?;
            reader.set_nonblocking(true)?;
            writer.write_all(partial)?;
            let started = Instant::now();
            let error = read_window_info(&mut reader, Duration::from_millis(20))
                .expect_err("A response is required");
            assert!(
                error
                    .to_string()
                    .contains("Timeout waiting for KWin status")
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
        Ok(())
    }
}
