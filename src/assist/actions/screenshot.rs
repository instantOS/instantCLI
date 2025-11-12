use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

pub fn screenshot_annotate() -> Result<()> {
    use crate::common::display_server::DisplayServer;

    let display_server = DisplayServer::detect();

    if display_server.is_wayland() {
        let flameshot_running = Command::new("pgrep")
            .arg("flameshot")
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);

        if !flameshot_running {
            Command::new("flameshot")
                .env("SDL_VIDEODRIVER", "wayland")
                .env("_JAVA_AWT_WM_NONREPARENTING", "1")
                .env("QT_QPA_PLATFORM", "wayland")
                .env("XDG_CURRENT_DESKTOP", "sway")
                .env("XDG_SESSION_DESKTOP", "sway")
                .spawn()
                .context("Failed to start flameshot daemon")?;

            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        Command::new("flameshot")
            .arg("gui")
            .env("SDL_VIDEODRIVER", "wayland")
            .env("_JAVA_AWT_WM_NONREPARENTING", "1")
            .env("QT_QPA_PLATFORM", "wayland")
            .env("XDG_CURRENT_DESKTOP", "sway")
            .env("XDG_SESSION_DESKTOP", "sway")
            .spawn()
            .context("Failed to launch flameshot gui")?;
    } else {
        std::thread::sleep(std::time::Duration::from_millis(100));

        Command::new("flameshot")
            .arg("gui")
            .spawn()
            .context("Failed to launch flameshot gui")?;
    }

    Ok(())
}

pub fn screenshot_to_clipboard() -> Result<()> {
    use crate::common::display_server::DisplayServer;

    let display_server = DisplayServer::detect();

    if display_server.is_wayland() {
        let slurp_output = Command::new("slurp")
            .output()
            .context("Failed to run slurp for area selection")?;

        if !slurp_output.status.success() {
            return Ok(());
        }

        let geometry = String::from_utf8_lossy(&slurp_output.stdout)
            .trim()
            .to_string();

        if geometry.is_empty() {
            return Ok(());
        }

        let grim_output = Command::new("grim")
            .args(["-g", &geometry, "-"])
            .output()
            .context("Failed to capture screenshot with grim")?;

        if !grim_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        let mut wl_copy = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start wl-copy")?;

        if let Some(mut stdin) = wl_copy.stdin.take() {
            stdin
                .write_all(&grim_output.stdout)
                .context("Failed to write screenshot to wl-copy")?;
        }

        wl_copy.wait().context("Failed to wait for wl-copy")?;
    } else if display_server.is_x11() {
        let slop_output = Command::new("slop")
            .arg("-f")
            .arg("%g")
            .output()
            .context("Failed to run slop for area selection")?;

        if !slop_output.status.success() {
            return Ok(());
        }

        let geometry = String::from_utf8_lossy(&slop_output.stdout)
            .trim()
            .to_string();

        if geometry.is_empty() {
            return Ok(());
        }

        let import_output = Command::new("import")
            .args(["-window", "root", "-crop", &geometry, "png:-"])
            .output()
            .context("Failed to capture screenshot with import")?;

        if !import_output.status.success() {
            anyhow::bail!("Failed to capture screenshot");
        }

        let mut xclip = Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "image/png"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start xclip")?;

        if let Some(mut stdin) = xclip.stdin.take() {
            stdin
                .write_all(&import_output.stdout)
                .context("Failed to write screenshot to xclip")?;
        }

        xclip.wait().context("Failed to wait for xclip")?;
    } else {
        anyhow::bail!("Unknown display server - cannot take screenshot");
    }

    Ok(())
}

pub fn screenshot_to_imgur() -> Result<()> {
    use crate::common::display_server::DisplayServer;
    
    let display_server = DisplayServer::detect();
    
    if display_server.is_wayland() {
        let slurp_output = Command::new("slurp")
            .output()
            .context("Failed to run slurp for area selection")?;

        if !slurp_output.status.success() {
            return Ok(());
        }

        let geometry = String::from_utf8_lossy(&slurp_output.stdout)
            .trim()
            .to_string();

        if geometry.is_empty() {
            return Ok(());
        }

        let grim_child = Command::new("grim")
            .args(["-g", &geometry, "-"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start grim")?;

        let curl_child = Command::new("curl")
            .args([
                "-s",
                "-F", "image=@-",
                "https://api.imgur.com/3/image",
                "-H", "Authorization: Client-ID 546c25a59c58ad7",
            ])
            .stdin(grim_child.stdout.unwrap())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start curl")?;

        let curl_output = curl_child
            .wait_with_output()
            .context("Failed to wait for curl")?;

        if !curl_output.status.success() {
            anyhow::bail!("Failed to upload screenshot to Imgur");
        }

        let mut jq_child = Command::new("jq")
            .args(["-r", ".data.link"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start jq")?;

        let mut jq_stdin = jq_child.stdin.take().unwrap();
        jq_stdin
            .write_all(&curl_output.stdout)
            .context("Failed to write to jq")?;
        drop(jq_stdin);

        let jq_output = jq_child
            .wait_with_output()
            .context("Failed to wait for jq")?;

        if !jq_output.status.success() {
            anyhow::bail!("Failed to parse Imgur response");
        }

        let imgur_link = String::from_utf8_lossy(&jq_output.stdout)
            .trim()
            .to_string();

        if imgur_link.is_empty() {
            anyhow::bail!("Failed to extract Imgur link from response");
        }

        let mut wl_copy = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start wl-copy")?;

        if let Some(mut stdin) = wl_copy.stdin.take() {
            stdin
                .write_all(imgur_link.as_bytes())
                .context("Failed to write Imgur link to wl-copy")?;
        }

        wl_copy.wait().context("Failed to wait for wl-copy")?;

        Command::new("notify-send")
            .args(["Imgur link copied", &imgur_link])
            .spawn()
            .context("Failed to show notification")?;

        Ok(())
    } else {
        anyhow::bail!("Imgur upload currently only supports Wayland");
    }
}
