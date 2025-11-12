use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

pub fn qr_encode_clipboard() -> Result<()> {
    use crate::common::display_server::DisplayServer;

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

    let temp_content =
        std::env::temp_dir().join(format!("qr_content_{}.txt", std::process::id()));
    std::fs::write(&temp_content, clipboard_text.as_bytes())
        .context("Failed to write clipboard content to temp file")?;

    let temp_script =
        std::env::temp_dir().join(format!("qr_display_{}.sh", std::process::id()));
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
