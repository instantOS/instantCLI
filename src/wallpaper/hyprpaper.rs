use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Apply wallpaper using hyprpaper (native Hyprland wallpaper daemon).
///
/// This is the idiomatic way on modern Hyprland (0.45+). It uses
/// `hyprctl hyprpaper wallpaper` IPC, which is persistent and does not
/// depend on the parent terminal staying open (unlike the old `awww` path
/// that required careful daemonization).
///
/// Falls back to `awww` if `hyprpaper` is not installed.
pub fn apply_wallpaper(path: &str) -> Result<()> {
    // Prefer hyprpaper if available
    if which::which("hyprpaper").is_ok() {
        if let Err(e) = apply_via_hyprpaper(path) {
            eprintln!("hyprpaper failed ({}), falling back to awww: {}", e, path);
            // Fallback to awww for e.g. hyprpaper not running or IPC error
            return crate::wallpaper::awww::apply_wallpaper(path);
        }
        return Ok(());
    }

    // hyprpaper not installed - use awww
    crate::wallpaper::awww::apply_wallpaper(path)
}

fn apply_via_hyprpaper(path: &str) -> Result<()> {
    let abs_path = Path::new(path)
        .canonicalize()
        .context("Failed to resolve absolute path for wallpaper")?;
    let abs_str = abs_path.to_string_lossy().to_string();

    // Ensure daemon is running. `hyprctl hyprpaper listactive` succeeds only if daemon is up.
    let list = Command::new("hyprctl")
        .args(["hyprpaper", "listactive"])
        .output()
        .context("Failed to query hyprpaper")?;

    let daemon_running = list.status.success();

    if !daemon_running {
        // Start hyprpaper detached. Use nohup + setsid so closing the terminal
        // doesn't SIGHUP the daemon (same issue we fixed for awww).
        // Hyprland's autostart also does `exec-once = hyprpaper`, but we start on-demand.
        Command::new("sh")
            .args(["-c", "nohup hyprpaper >/dev/null 2>&1 &"])
            .output()
            .context("Failed to start hyprpaper daemon")?;
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // Hyprland 0.8+ uses `hyprctl hyprpaper wallpaper "mon,path,fit"` where mon can be empty for fallback.
    // Use ",<path>" to set on all monitors. Try "cover" fit which matches awww's `crop`.
    let wallpaper_arg = format!(",{}", abs_str);
    let output = Command::new("hyprctl")
        .args(["hyprpaper", "wallpaper", &wallpaper_arg])
        .output()
        .context("Failed to set wallpaper with hyprctl hyprpaper")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // hyprpaper IPC returns error in stdout/stderr, try fallback to preload+wallpaper old syntax
        let msg = format!("{} {}", stdout, stderr);
        if msg.contains("invalid") || msg.contains("not running") || msg.contains("error") {
            // Try old preload flow: `hyprctl hyprpaper preload <path>` then wallpaper
            let _ = Command::new("hyprctl")
                .args(["hyprpaper", "preload", &abs_str])
                .output();
            std::thread::sleep(std::time::Duration::from_millis(200));
            let retry = Command::new("hyprctl")
                .args(["hyprpaper", "wallpaper", &wallpaper_arg])
                .output()
                .context("Failed to set wallpaper with hyprctl hyprpaper (retry)")?;
            if !retry.status.success() {
                let s = String::from_utf8_lossy(&retry.stderr);
                let o = String::from_utf8_lossy(&retry.stdout);
                anyhow::bail!("hyprpaper failed to set wallpaper: {} {}", o, s);
            }
            return Ok(());
        }
        anyhow::bail!("hyprpaper failed to set wallpaper: {}", msg.trim());
    }

    Ok(())
}
