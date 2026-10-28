use super::ScratchpadProvider;
use super::ScratchpadWindowInfo;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
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
            .args([&format!("destination='{}'", monitor_dest)])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn dbus-monitor")?;

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

        // Run the script
        // We use a separate thread or just run it. dbus-monitor is running in background.
        // We need to run the script loading logic here.
        // We can reuse the logic from run_script but simpler since we don't need args.

        let run_result = self.execute_kwin_script(&script_content);

        if run_result.is_err() {
            let _ = monitor.kill();
            return Err(run_result.err().unwrap());
        }

        // Read dbus-monitor output
        // We expect a method call with the JSON string
        let start = Instant::now();
        let timeout = Duration::from_millis(500); // 500ms timeout
        let mut json_str = String::new();
        let mut found = false;

        if let Some(stdout) = monitor.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if start.elapsed() > timeout {
                    break;
                }

                if let Ok(l) = line
                    && l.contains("string \"[{")
                {
                    // This looks like our JSON array
                    // dbus-monitor output format for string is: string "..."
                    // We need to extract the content inside quotes and unescape it
                    if let Some(start_idx) = l.find("string \"") {
                        let content = &l[start_idx + 8..];
                        if let Some(content) = content.strip_suffix('"') {
                            json_str = content.to_string();
                            found = true;
                            break;
                        }
                    }
                }
            }
        }

        let _ = monitor.kill();

        if !found {
            return Err(anyhow::anyhow!("Timeout waiting for KWin status"));
        }

        // dbus-monitor escapes quotes as \", we need to unescape
        let json_str = json_str.replace("\\\"", "\"");

        let windows: Vec<KWinWindowInfo> =
            serde_json::from_str(&json_str).context("Failed to parse KWin window info JSON")?;

        Ok(windows)
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
                "--dest=org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                &format!("string:{}", path),
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to load script"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let id_str = stdout.lines().last().unwrap_or("").trim();
        let id_parts: Vec<&str> = id_str.split_whitespace().collect();
        let id = if id_parts.len() >= 2 && id_parts[0] == "int32" {
            id_parts[1]
        } else {
            return Err(anyhow::anyhow!("Invalid script ID"));
        };

        let script_obj_path = format!("/Scripting/Script{}", id);
        Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.run",
            ])
            .output()?;

        // Stop it immediately
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.stop",
            ])
            .output();

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

        // Create temp file
        let mut temp_file = Builder::new()
            .prefix("kwin-scratchpad-")
            .suffix(".js")
            .tempfile()?;

        write!(temp_file, "{}", script_content)?;
        temp_file.flush()?;
        let path = temp_file.path().to_str().context("Invalid path")?;

        // DBus call to load script
        let output = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                &format!("string:{}", path),
            ])
            .output()
            .context("Failed to execute dbus-send to load script")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to load KWin script: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Parse ID from output. Output format:
        // method return time=... sender=... -> destination=... serial=... reply_serial=...
        //    int32 4
        let stdout = String::from_utf8_lossy(&output.stdout);
        let id_str = stdout.lines().last().unwrap_or("").trim();
        // Extract number
        let id_parts: Vec<&str> = id_str.split_whitespace().collect();
        let id = if id_parts.len() >= 2 && id_parts[0] == "int32" {
            id_parts[1]
        } else {
            return Err(anyhow::anyhow!(
                "Unexpected response from loadScript: {}",
                stdout
            ));
        };

        // Run script
        let script_obj_path = format!("/Scripting/Script{}", id);
        Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.run",
            ])
            .output()
            .context("Failed to run KWin script")?;

        // Cleanup: Stop/Unload script?
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.stop",
            ])
            .output();

        Ok(())
    }
}
