use super::ScratchpadProvider;
use super::ScratchpadWindowInfo;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;
use tempfile::Builder;

pub struct KWin;

impl ScratchpadProvider for KWin {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        // Try to show window first (fast if exists)
        self.run_script(config, "show")?;

        // Check if window exists - if not, create it
        // We do this check AFTER trying to show to optimize for the common case
        if !self.is_window_running(config)? {
            super::create_terminal_process(config)?;
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
            super::create_terminal_process(config)?;
            std::thread::sleep(std::time::Duration::from_millis(700));
            self.run_script(config, "show")?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        // Use KWin scripting to find all scratchpad windows
        let script_content = r#"
            (function() {
                const results = [];
                const clients = workspace.windowList();
                
                for (let i = 0; i < clients.length; i++) {
                    const client = clients[i];
                    if (client.resourceClass && client.resourceClass.indexOf("scratchpad_") !== -1) {
                        results.push({
                            name: client.resourceClass.replace("scratchpad_", ""),
                            windowClass: client.resourceClass,
                            title: client.caption || "",
                            visible: !client.minimized && client.desktops.indexOf(workspace.currentDesktop) !== -1
                        });
                    }
                }
                return results;
            })();
        "#;

        let mut temp_file = Builder::new()
            .prefix("kwin-list-")
            .suffix(".js")
            .tempfile()?;

        write!(temp_file, "{}", script_content)?;
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
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let id_str = stdout.lines().last().unwrap_or("").trim();
        let id_parts: Vec<&str> = id_str.split_whitespace().collect();

        let id = if id_parts.len() >= 2 && id_parts[0] == "int32" {
            id_parts[1]
        } else {
            return Ok(Vec::new());
        };

        let script_obj_path = format!("/Scripting/Script{}", id);

        // Since we can't easily get return values from dbus-send,
        // fall back to process-based detection for now
        Ok(Vec::new())
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        let class = config.window_class();

        // Use terminal process check - faster than KWin scripting
        // Look for the terminal process with our specific class
        let terminal_cmd = config.terminal.command();
        let output = Command::new("pgrep")
            .arg("-f")
            .arg(format!("{}.*{}", terminal_cmd, class))
            .output()?;

        Ok(output.status.success())
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        let class = config.window_class();

        // Use KWin scripting to check visibility
        let script_content = format!(
            r#"
            (function() {{
                const clients = workspace.windowList();
                for (let i = 0; i < clients.length; i++) {{
                    const client = clients[i];
                    if (client.resourceClass === "{class}") {{
                        return (!client.minimized && client.desktops.indexOf(workspace.currentDesktop) !== -1).toString();
                    }}
                }}
                return "false";
            }})();
            "#
        );

        let mut temp_file = Builder::new()
            .prefix("kwin-visible-")
            .suffix(".js")
            .tempfile()?;

        write!(temp_file, "{}", script_content)?;
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
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let id_str = stdout.lines().last().unwrap_or("").trim();
        let id_parts: Vec<&str> = id_str.split_whitespace().collect();

        let id = if id_parts.len() >= 2 && id_parts[0] == "int32" {
            id_parts[1]
        } else {
            return Ok(false);
        };

        let script_obj_path = format!("/Scripting/Script{}", id);

        // For now, return false as we can't easily get return values
        // This will be improved when we have better communication
        Ok(false)
    }
}

impl KWin {
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
