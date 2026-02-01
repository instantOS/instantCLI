use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn apply_wallpaper(path: &str) -> Result<()> {
    let abs_path = Path::new(path)
        .canonicalize()
        .context("Failed to resolve absolute path for wallpaper")?;
    let path_str = abs_path.to_string_lossy();

    // Try plasma-apply-wallpaperimage first
    if Command::new("plasma-apply-wallpaperimage")
        .arg(&*path_str)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    // Fallback to qdbus
    let script = format!(
        r#"
        desktops().forEach(d => {{
            d.currentConfigGroup = Array("Wallpaper", "org.kde.image", "General");
            d.writeConfig("Image", "file://{}");
            d.reloadConfig();
        }});
        "#,
        path_str.replace('\\', "\\\\").replace('"', "\\\"")
    );

    // Try qdbus6 (Plasma 6)
    if run_qdbus_script("qdbus6", &script).is_ok() {
        return Ok(());
    }

    // Try qdbus (Plasma 5)
    if run_qdbus_script("qdbus", &script).is_ok() {
        return Ok(());
    }

    // Try qdbus-qt5 (Plasma 5 on some distros)
    if run_qdbus_script("qdbus-qt5", &script).is_ok() {
        return Ok(());
    }

    anyhow::bail!(
        "Failed to set KDE wallpaper: neither plasma-apply-wallpaperimage nor qdbus found/worked"
    )
}

fn run_qdbus_script(cmd: &str, script: &str) -> Result<()> {
    Command::new(cmd)
        .args([
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell.evaluateScript",
            script,
        ])
        .output()
        .context("Failed to run qdbus script")
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                anyhow::bail!("qdbus command returned error")
            }
        })
}
