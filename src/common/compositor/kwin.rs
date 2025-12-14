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
        self.run_script(config, "show")
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        self.run_script(config, "hide")
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        self.run_script(config, "toggle")
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        // Implementing this properly requires bi-directional communication which is hard with dbus-send.
        // For now, return empty list or fallback implementation logic if we can.
        Ok(Vec::new())
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        // Check using pgrep as fallback
        let class = config.window_class();
        let output = Command::new("pgrep")
            .arg("-f")
            .arg(&class)
            .output()?;

        Ok(output.status.success())
    }

    fn is_visible(&self, _config: &ScratchpadConfig) -> Result<bool> {
        // Without bi-directional communication, we can't know for sure.
        // Return Ok(false) so that `toggle` logic in the script (which knows the truth) can handle it?
        Ok(false)
    }
}

impl KWin {
    fn run_script(&self, config: &ScratchpadConfig, action: &str) -> Result<()> {
        let class = config.window_class();

        // Plasma 6 Script
        // Uses `workspace.windows` (QList<KWin::Window*>)
        // `client.resourceClass` (property)
        // `client.minimized` (property)
        // `client.desktops` (property)

        let script_content = format!(r#"
            (function() {{
                const clients = workspace.windows;
                const targetClass = "{class}";
                const action = "{action}";

                for (const client of clients) {{
                    if (client.resourceClass === targetClass) {{
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
                        return;
                    }}
                }}
            }})();
        "#);

        // Create temp file
        let mut temp_file = Builder::new()
            .prefix("kwin-scratchpad-")
            .suffix(".js")
            .tempfile()?;

        write!(temp_file, "{}", script_content)?;
        let path = temp_file.path().to_str().context("Invalid path")?;

        // DBus call to load script
        let output = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                &format!("string:{}", path)
            ])
            .output()
            .context("Failed to execute dbus-send to load script")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to load KWin script: {}", String::from_utf8_lossy(&output.stderr)));
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
            return Err(anyhow::anyhow!("Unexpected response from loadScript: {}", stdout));
        };

        // Run script
        let script_obj_path = format!("/Scripting/Script{}", id);
        Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.run"
            ])
            .output()
            .context("Failed to run KWin script")?;

        // Cleanup: Stop/Unload script?
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.kde.KWin",
                &script_obj_path,
                "org.kde.kwin.Script.stop"
            ])
            .output();

        Ok(())
    }
}
