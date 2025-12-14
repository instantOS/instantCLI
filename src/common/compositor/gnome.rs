use super::{create_terminal_process, ScratchpadProvider, ScratchpadWindowInfo};
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

pub struct Gnome;

#[derive(Debug, Deserialize)]
struct WindowInfo {
    id: u64,
    wm_class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    focus: bool,
}

impl Gnome {
    fn call_dbus_method(method: &str, args: &[String]) -> Result<String> {
        let mut cmd = Command::new("gdbus");
        cmd.arg("call")
            .arg("--session")
            .arg("--dest")
            .arg("org.gnome.Shell")
            .arg("--object-path")
            .arg("/org/gnome/Shell/Extensions/Windows")
            .arg("--method")
            .arg(format!("org.gnome.Shell.Extensions.Windows.{}", method));

        for arg in args {
            cmd.arg(arg);
        }

        cmd.arg("--print-reply=literal");

        let output = cmd.output().context("Failed to execute gdbus")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Unknown method")
                || stderr.contains("Service was not found")
                || stderr.contains("Method name")
                || stderr.contains("Object does not exist")
            {
                return Err(anyhow::anyhow!(
                    "Gnome Scratchpad requires the 'Window Calls' extension. Please install it: https://extensions.gnome.org/extension/4724/window-calls/"
                ));
            }
            return Err(anyhow::anyhow!("gdbus call failed: {}", stderr));
        }

        let stdout = String::from_utf8(output.stdout).context("Invalid UTF-8 from gdbus")?;
        Ok(stdout.trim().to_string())
    }

    fn list_windows() -> Result<Vec<WindowInfo>> {
        let json_str = Self::call_dbus_method("List", &[])?;
        if json_str.is_empty() {
            return Ok(Vec::new());
        }

        let windows: Vec<WindowInfo> = serde_json::from_str(&json_str)
            .context(format!("Failed to parse window list JSON: {}", json_str))?;
        Ok(windows)
    }

    fn find_window(window_class: &str) -> Result<Option<WindowInfo>> {
        let windows = Self::list_windows()?;
        Ok(windows.into_iter().find(|w| w.wm_class == window_class))
    }
}

impl ScratchpadProvider for Gnome {
    fn show(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        if let Some(window) = Self::find_window(&window_class)? {
            // Unminimize and Activate
            Self::call_dbus_method("Unminimize", &[window.id.to_string()])?;
            Self::call_dbus_method("Activate", &[window.id.to_string()])?;
        } else {
            create_terminal_process(config)?;
        }
        Ok(())
    }

    fn hide(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        if let Some(window) = Self::find_window(&window_class)? {
            Self::call_dbus_method("Minimize", &[window.id.to_string()])?;
        }
        Ok(())
    }

    fn toggle(&self, config: &ScratchpadConfig) -> Result<()> {
        let window_class = config.window_class();
        if let Some(window) = Self::find_window(&window_class)? {
            if window.focus {
                Self::call_dbus_method("Minimize", &[window.id.to_string()])?;
            } else {
                Self::call_dbus_method("Unminimize", &[window.id.to_string()])?;
                Self::call_dbus_method("Activate", &[window.id.to_string()])?;
            }
        } else {
            create_terminal_process(config)?;
        }
        Ok(())
    }

    fn get_all_windows(&self) -> Result<Vec<ScratchpadWindowInfo>> {
        let windows = Self::list_windows()?;
        let mut scratchpads = Vec::new();

        for window in windows {
            if let Some(name) = window.wm_class.strip_prefix("scratchpad_") {
                scratchpads.push(ScratchpadWindowInfo {
                    name: name.to_string(),
                    window_class: window.wm_class.clone(),
                    title: window.title.clone(),
                    visible: window.focus, // Best approximation
                });
            }
        }
        Ok(scratchpads)
    }

    fn is_window_running(&self, config: &ScratchpadConfig) -> Result<bool> {
        let window_class = config.window_class();
        Ok(Self::find_window(&window_class)?.is_some())
    }

    fn is_visible(&self, config: &ScratchpadConfig) -> Result<bool> {
        let window_class = config.window_class();
        if let Some(window) = Self::find_window(&window_class)? {
            Ok(window.focus)
        } else {
            Ok(false)
        }
    }
}
