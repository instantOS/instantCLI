use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

use crate::assist::utils::{
    AreaSelectionConfig, capture_area_to_file, copy_to_clipboard, show_notification,
};
use crate::common::display_server::DisplayServer;
use crate::common::paths;

pub fn qr_encode_clipboard() -> Result<()> {
    let display_server = DisplayServer::detect();

    let (clipboard_cmd, clipboard_args) = display_server.get_clipboard_command();
    let clipboard_content = Command::new(clipboard_cmd)
        .args(clipboard_args)
        .output()
        .with_context(|| format!("Failed to get clipboard with {}", clipboard_cmd))?
        .stdout;

    let clipboard_text = String::from_utf8_lossy(&clipboard_content);

    if clipboard_text.trim().is_empty() {
        anyhow::bail!("Clipboard is empty");
    }

    let temp_content = std::env::temp_dir().join(format!("qr_content_{}.txt", std::process::id()));
    std::fs::write(&temp_content, clipboard_text.as_bytes())
        .context("Failed to write clipboard content to temp file")?;

    let temp_script = std::env::temp_dir().join(format!("qr_display_{}.sh", std::process::id()));
    let mut script =
        std::fs::File::create(&temp_script).context("Failed to create temporary script")?;

    writeln!(script, "#!/bin/bash")?;
    writeln!(script, "echo 'QR Code for clipboard contents:'")?;
    writeln!(script, "echo")?;
    writeln!(
        script,
        "cat '{}' | qrencode -t ansiutf8",
        temp_content.display()
    )?;
    writeln!(script, "echo")?;
    writeln!(script, "echo 'Press any key to close...'")?;
    writeln!(script, "read -n 1")?;
    writeln!(
        script,
        "rm -f '{}' '{}'",
        temp_content.display(),
        temp_script.display()
    )?;

    drop(script);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_script, perms)?;
    }

    let terminal = crate::common::terminal::detect_terminal();

    Command::new(terminal)
        .arg("-e")
        .arg(temp_script.as_os_str())
        .spawn()
        .context("Failed to launch terminal with QR code")?;

    Ok(())
}

pub fn qr_scan() -> Result<()> {
    let config = AreaSelectionConfig::new();

    let geometry = match config.select_area() {
        Ok(geom) => geom,
        Err(_) => return Ok(()),
    };

    let display_server = config.display_server();

    let pictures_dir = paths::pictures_dir().context("Failed to determine pictures directory")?;
    let image_path = pictures_dir.join("qrcode.png");

    capture_area_to_file(&geometry, &image_path, display_server)?;

    let zbarimg_output = Command::new("zbarimg")
        .arg("-q")
        .arg(&image_path)
        .output()
        .context("Failed to run zbarimg")?;

    if !zbarimg_output.status.success() {
        show_notification("No QR code detected", "")?;
        return Ok(());
    }

    let raw_output = String::from_utf8_lossy(&zbarimg_output.stdout);
    let detected_text = raw_output
        .lines()
        .filter_map(|line| line.split_once(':').map(|(_, text)| text))
        .collect::<Vec<_>>()
        .join("\n");

    if detected_text.is_empty() {
        show_notification("No QR code detected", "")?;
        return Ok(());
    }

    copy_to_clipboard(detected_text.as_bytes(), display_server)?;

    show_notification("Read QR code text", &detected_text)?;

    Ok(())
}
