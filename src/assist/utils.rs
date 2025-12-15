/// Shared utility functions for assists
use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, ExitStatus};

use crate::common::display_server::DisplayServer;

/// Launch a command in a detached terminal window
///
/// Auto-detects the user's preferred terminal emulator.
pub fn launch_in_terminal(command: &str) -> Result<()> {
    crate::common::terminal::TerminalLauncher::new("bash")
        .title("InstantCLI Assist")
        .args(&["-c".to_string(), command.to_string()])
        .launch()
}

/// Launch a script in a detached terminal window with title
///
/// Auto-detects the user's preferred terminal emulator.
/// Note: Title support varies by terminal emulator.
pub fn launch_script_in_terminal(script: &str, title: &str) -> Result<()> {
    use tempfile::NamedTempFile;

    // Write script to temporary file
    let mut temp_file = NamedTempFile::new().context("Failed to create temporary script file")?;
    temp_file
        .write_all(script.as_bytes())
        .context("Failed to write script")?;

    let script_path = temp_file.path().to_owned();

    // Make it executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms)?;
    }

    crate::common::terminal::TerminalLauncher::new("bash")
        .title(title)
        .arg(script_path.to_string_lossy())
        .launch()?;

    // Keep temp file alive by forgetting it (will be cleaned up by OS)
    std::mem::forget(temp_file);

    Ok(())
}

/// Run a script in a terminal window and wait for completion
///
/// Auto-detects the user's preferred terminal emulator.
/// Note: Hold/wait behavior varies by terminal emulator.
pub fn run_script_in_terminal_and_wait(script: &str, title: &str) -> Result<ExitStatus> {
    use tempfile::NamedTempFile;

    let mut temp_file = NamedTempFile::new().context("Failed to create temporary script file")?;
    temp_file
        .write_all(script.as_bytes())
        .context("Failed to write script")?;

    let script_path = temp_file.path().to_owned();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms)?;
    }

    let terminal = crate::common::terminal::detect_terminal();
    let exec_flag = crate::common::terminal::get_execute_flag(&terminal);

    let mut cmd = Command::new(&terminal);

    // Add terminal-specific flags for hold/wait behavior and title
    match terminal.as_str() {
        "kitty" => {
            cmd.arg("--hold").arg("--title").arg(title);
        }
        "alacritty" | "wezterm" => {
            cmd.arg("--title").arg(title);
        }
        _ => {
            // Other terminals may not support these flags
        }
    }

    let status = cmd
        .arg(exec_flag)
        .arg("bash")
        .arg(&script_path)
        .status()
        .context("Failed to run terminal command")?;

    Ok(status)
}

/// Launch a command in the background (detached)
#[allow(dead_code)]
pub fn launch_detached(program: &str, args: &[&str]) -> Result<()> {
    Command::new(program)
        .args(args)
        .spawn()
        .context(format!("Failed to launch {} in background", program))?;
    Ok(())
}

/// Get the current executable path (useful for calling self)
pub fn current_exe() -> Result<std::path::PathBuf> {
    std::env::current_exe().context("Failed to get current executable path")
}

/// Execute an ins menu command
pub fn menu_command(args: &[&str]) -> Result<()> {
    Command::new(current_exe()?)
        .arg("menu")
        .args(args)
        .spawn()
        .context("Failed to execute menu command")?;
    Ok(())
}

/// Area selection color configuration
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum ColorConfiguration {
    Red,
    Green,
    Blue,
    Yellow,
    Default,
}

impl ColorConfiguration {
    /// Get hex color for Wayland (slurp)
    pub fn wayland_hex(self) -> &'static str {
        match self {
            Self::Red => "#F38B82",
            Self::Green => "#81C995",
            Self::Blue => "#89B3F7",
            Self::Yellow => "#FDD663",
            Self::Default => "#81C995", // Default to green
        }
    }

    /// Get RGB color for X11 (slop)
    pub fn x11_rgb(self) -> &'static str {
        match self {
            Self::Red => "0.9529411764705882,0.5450980392156862,0.5098039215686274",
            Self::Green => "0.5058823529411764,0.788235294117647,0.5843137254901961",
            Self::Blue => "0.5372549019607843,0.7019607843137254,0.9686274509803922",
            Self::Yellow => "0.9921568627450981,0.8392156862745098,0.38823529411764707",
            Self::Default => "0.5058823529411764,0.788235294117647,0.5843137254901961", // Default to green
        }
    }
}

/// Cached display server and compositor detection
#[derive(Debug, Clone)]
pub struct AreaSelectionConfig {
    display_server: DisplayServer,
    has_compositor: bool,
    color: ColorConfiguration,
}

impl AreaSelectionConfig {
    /// Create new configuration with default green color
    pub fn new() -> Self {
        Self {
            display_server: DisplayServer::detect(),
            has_compositor: Self::detect_compositor(),
            color: ColorConfiguration::Default,
        }
    }

    /// Create configuration with custom color
    #[allow(dead_code)]
    pub fn with_color(color: ColorConfiguration) -> Self {
        Self {
            display_server: DisplayServer::detect(),
            has_compositor: Self::detect_compositor(),
            color,
        }
    }

    /// Detect if any compositing manager is running (X11 only)
    fn detect_compositor() -> bool {
        // Common compositors to check for
        let compositors = ["picom", "compton", "xcompmgr", "unagi"];

        for compositor in &compositors {
            if let Ok(output) = Command::new("pgrep").arg(compositor).output()
                && !output.stdout.is_empty()
            {
                return true;
            }
        }
        false
    }

    /// Select screen area using appropriate tools for the current display server
    /// Returns geometry string that can be used with screenshot tools
    pub fn select_area(&self) -> Result<String> {
        match self.display_server {
            DisplayServer::Wayland => self.select_area_wayland(),
            DisplayServer::X11 => self.select_area_x11(),
            _ => anyhow::bail!("Unsupported display server for area selection"),
        }
    }

    /// Select area on Wayland using slurp with configurable color
    fn select_area_wayland(&self) -> Result<String> {
        let color_hex = self.color.wayland_hex();

        let mut cmd = Command::new("slurp");
        if !color_hex.is_empty() {
            cmd.arg("-c").arg(color_hex);
        }

        let output = cmd
            .output()
            .context("Failed to run slurp for area selection")?;

        if !output.status.success() {
            anyhow::bail!("Area selection cancelled");
        }

        let geometry = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if geometry.is_empty() {
            anyhow::bail!("No area selected");
        }

        Ok(geometry)
    }

    /// Select area on X11 using slop with configurable color and compositing support
    fn select_area_x11(&self) -> Result<String> {
        let rgb_color = self.color.x11_rgb();

        let mut cmd = Command::new("slop");

        // Add compositing-aware options
        if self.has_compositor {
            cmd.arg("--highlight");
            cmd.arg("-b").arg("3");
            cmd.arg("-c").arg(format!("{},0.1", rgb_color));
        } else {
            cmd.arg("-b").arg("3");
            cmd.arg("-c").arg(rgb_color);
        }

        cmd.arg("-f").arg("%g");

        let output = cmd
            .output()
            .context("Failed to run slop for area selection")?;

        if !output.status.success() {
            anyhow::bail!("Area selection cancelled");
        }

        let geometry = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if geometry.is_empty() {
            anyhow::bail!("No area selected");
        }

        Ok(geometry)
    }

    /// Get the display server for this configuration
    pub fn display_server(&self) -> &DisplayServer {
        &self.display_server
    }
}

/// Copy data to clipboard using the appropriate tool for the display server
pub fn copy_to_clipboard(data: &[u8], display_server: &DisplayServer) -> Result<()> {
    if matches!(*display_server, DisplayServer::Wayland) {
        let mut wl_copy = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start wl-copy")?;

        if let Some(mut stdin) = wl_copy.stdin.take() {
            stdin
                .write_all(data)
                .context("Failed to write to wl-copy")?;
        }

        wl_copy.wait().context("Failed to wait for wl-copy")?;
    } else if matches!(*display_server, DisplayServer::X11) {
        let mut xclip = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start xclip")?;

        if let Some(mut stdin) = xclip.stdin.take() {
            stdin.write_all(data).context("Failed to write to xclip")?;
        }

        xclip.wait().context("Failed to wait for xclip")?;
    } else {
        anyhow::bail!("Unknown display server - cannot copy to clipboard");
    }

    Ok(())
}

/// Copy image data to clipboard with explicit MIME type (X11 only)
pub fn copy_image_to_clipboard(
    data: &[u8],
    mime_type: &str,
    display_server: &DisplayServer,
) -> Result<()> {
    if matches!(*display_server, DisplayServer::Wayland) {
        let mut wl_copy = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start wl-copy")?;

        if let Some(mut stdin) = wl_copy.stdin.take() {
            stdin
                .write_all(data)
                .context("Failed to write to wl-copy")?;
        }

        wl_copy.wait().context("Failed to wait for wl-copy")?;
    } else if matches!(*display_server, DisplayServer::X11) {
        let mut xclip = Command::new("xclip")
            .args(["-selection", "clipboard", "-t", mime_type])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start xclip")?;

        if let Some(mut stdin) = xclip.stdin.take() {
            stdin.write_all(data).context("Failed to write to xclip")?;
        }

        xclip.wait().context("Failed to wait for xclip")?;
    } else {
        anyhow::bail!("Unknown display server - cannot copy to clipboard");
    }

    Ok(())
}

/// Capture screenshot of selected area to memory (as PNG bytes)
pub fn capture_area_to_memory(geometry: &str, display_server: &DisplayServer) -> Result<Vec<u8>> {
    if matches!(*display_server, DisplayServer::Wayland) {
        let grim_output = Command::new("grim")
            .args(["-g", geometry, "-"])
            .output()
            .context("Failed to capture screenshot with grim")?;

        if !grim_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        Ok(grim_output.stdout)
    } else if matches!(*display_server, DisplayServer::X11) {
        let import_output = Command::new("import")
            .args(["-window", "root", "-crop", geometry, "png:-"])
            .output()
            .context("Failed to capture screenshot with import")?;

        if !import_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        Ok(import_output.stdout)
    } else {
        anyhow::bail!("Unknown display server - cannot take screenshot")
    }
}

/// Capture screenshot of selected area to file
pub fn capture_area_to_file(
    geometry: &str,
    file_path: &std::path::Path,
    display_server: &DisplayServer,
) -> Result<()> {
    if matches!(*display_server, DisplayServer::Wayland) {
        let status = Command::new("grim")
            .args(["-g", geometry])
            .arg(file_path)
            .status()
            .context("Failed to capture screenshot with grim")?;

        if !status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }
    } else if matches!(*display_server, DisplayServer::X11) {
        let status = Command::new("import")
            .args(["-window", "root", "-crop", geometry])
            .arg(file_path)
            .status()
            .context("Failed to capture screenshot with import")?;

        if !status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }
    } else {
        anyhow::bail!("Unknown display server - cannot take screenshot");
    }

    Ok(())
}

/// Show a notification using notify-send
pub fn show_notification(title: &str, message: &str) -> Result<()> {
    Command::new("notify-send")
        .args(["-a", "instantASSIST", title, message])
        .spawn()
        .context("Failed to show notification")?;
    Ok(())
}

/// Show a notification with icon using notify-send
#[allow(dead_code)]
pub fn show_notification_with_icon(title: &str, message: &str, icon: &str) -> Result<()> {
    Command::new("notify-send")
        .args(["-a", "instantASSIST", "-i", icon, title, message])
        .spawn()
        .context("Failed to show notification")?;
    Ok(())
}

/// Generate screenshot filename with timestamp
pub fn generate_screenshot_filename() -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    format!("{}.png", timestamp)
}

/// Get text content from clipboard using the appropriate tool for the display server
pub fn get_clipboard_content(display_server: &DisplayServer) -> Result<String> {
    let output = if matches!(*display_server, DisplayServer::Wayland) {
        Command::new("wl-paste")
            .output()
            .context("Failed to run wl-paste")?
    } else if matches!(*display_server, DisplayServer::X11) {
        Command::new("xclip")
            .args(["-selection", "clipboard", "-o"])
            .output()
            .context("Failed to run xclip")?
    } else {
        anyhow::bail!("Unknown display server - cannot get clipboard content");
    };

    if !output.status.success() {
        anyhow::bail!("Failed to get clipboard content");
    }

    let content =
        String::from_utf8(output.stdout).context("Clipboard content is not valid UTF-8")?;

    if content.trim().is_empty() {
        anyhow::bail!("Clipboard is empty");
    }

    Ok(content)
}
