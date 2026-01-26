use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

use crate::assist::utils::{
    capture_area_to_memory, copy_to_clipboard, show_notification, show_notification_with_icon,
};
use crate::common::compositor::CompositorType;
use crate::common::display_server::DisplayServer;

/// Universal color picker using grim + slurp + imagemagick
fn pick_color_universal_wayland(display_server: &DisplayServer) -> Result<Option<String>> {
    // Use slurp to let user select a pixel
    // -p: point selection
    // -c: selection color
    // -f "%x,%y 1x1": force output format compatible with grim (x,y wxh) to capture single pixel
    let slurp_output = Command::new("slurp")
        .args(["-p", "-c", "#ff0000", "-f", "%x,%y 1x1"])
        .output()
        .context("Failed to run slurp for color selection")?;

    if !slurp_output.status.success() {
        // User cancelled selection
        return Ok(None);
    }

    // Capture screenshot of 1x1 pixel at selected coordinates
    let slurp_str = String::from_utf8_lossy(&slurp_output.stdout);
    let geometry = slurp_str.trim();

    // Capture the area to memory using grim (handled by utils)
    let screenshot_data = capture_area_to_memory(geometry, display_server)?;

    // Use ImageMagick to extract color from the single pixel
    // We use info:- with a specific format to get the hex color directly
    // -depth 8 ensures we get standard 8-bit per channel hex codes (e.g., #RRGGBB)
    let mut convert_child = Command::new("convert")
        .args(["-", "-depth", "8", "-format", "#%[hex:p{0,0}]", "info:-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("Failed to start ImageMagick convert")?;

    if let Some(stdin) = convert_child.stdin.as_mut() {
        stdin
            .write_all(&screenshot_data)
            .context("Failed to write screenshot data to ImageMagick")?;
    }

    let convert_output = convert_child
        .wait_with_output()
        .context("Failed to extract color with ImageMagick convert")?;

    if !convert_output.status.success() {
        // ImageMagick failed
        return Err(anyhow::anyhow!(
            "ImageMagick convert failed. Ensure ImageMagick is installed."
        ));
    }

    let output = String::from_utf8_lossy(&convert_output.stdout)
        .trim()
        .to_string();

    // Validate output
    if output.starts_with('#') {
        // Remove alpha channel if present (e.g., #RRGGBBAA -> #RRGGBB)
        // Standard length for #RRGGBB is 7
        if output.len() > 7 {
            return Ok(Some(output[0..7].to_string()));
        }
        return Ok(Some(output));
    }

    // Fallback: if output is just hex without #, add it
    if output.chars().all(|c| c.is_ascii_hexdigit()) && !output.is_empty() {
        let hex = if output.len() > 6 {
            &output[0..6]
        } else {
            &output
        };
        return Ok(Some(format!("#{}", hex)));
    }

    Err(anyhow::anyhow!(
        "Failed to parse color from output: {}",
        output
    ))
}

/// KDE-specific color picker using Spectacle and ImageMagick
fn pick_color_kde() -> Result<Option<String>> {
    // Use spectacle to capture a region to stdout
    // --background: don't show window
    // --nonotify: don't show notification
    // --region: let user select region (we want a single pixel, but region is closest interactive mode)
    // --output /proc/self/fd/1: write to stdout
    let spectacle_output = Command::new("spectacle")
        .args([
            "--background",
            "--nonotify",
            "--region",
            "--output",
            "/proc/self/fd/1",
        ])
        .output()
        .context("Failed to run spectacle for color selection")?;

    if !spectacle_output.status.success() {
        // User cancelled selection or failure
        return Ok(None);
    }

    let screenshot_data = spectacle_output.stdout;

    if screenshot_data.is_empty() {
        return Ok(None);
    }

    // Use ImageMagick to extract color from the image (average color if multiple pixels)
    // -depth 8 ensures we get standard 8-bit per channel hex codes (e.g., #RRGGBB)
    // -resize 1x1! forces it to a single pixel to get the average color of selection
    let mut convert_child = Command::new("convert")
        .args([
            "-",
            "-resize",
            "1x1!",
            "-depth",
            "8",
            "-format",
            "#%[hex:p{0,0}]",
            "info:-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("Failed to start ImageMagick convert")?;

    if let Some(stdin) = convert_child.stdin.as_mut() {
        stdin
            .write_all(&screenshot_data)
            .context("Failed to write screenshot data to ImageMagick")?;
    }

    let convert_output = convert_child
        .wait_with_output()
        .context("Failed to extract color with ImageMagick convert")?;

    if !convert_output.status.success() {
        // ImageMagick failed
        return Err(anyhow::anyhow!(
            "ImageMagick convert failed. Ensure ImageMagick is installed."
        ));
    }

    let output = String::from_utf8_lossy(&convert_output.stdout)
        .trim()
        .to_string();

    // Validate output
    if output.starts_with('#') {
        // Remove alpha channel if present (e.g., #RRGGBBAA -> #RRGGBB)
        // Standard length for #RRGGBB is 7
        if output.len() > 7 {
            return Ok(Some(output[0..7].to_string()));
        }
        return Ok(Some(output));
    }

    // Fallback: if output is just hex without #, add it
    if output.chars().all(|c| c.is_ascii_hexdigit()) && !output.is_empty() {
        let hex = if output.len() > 6 {
            &output[0..6]
        } else {
            &output
        };
        return Ok(Some(format!("#{}", hex)));
    }

    Err(anyhow::anyhow!(
        "Failed to parse color from output: {}",
        output
    ))
}

/// Try specialized color pickers first, fallback to universal approach
fn pick_color_with_fallbacks(display_server: &DisplayServer) -> Result<Option<String>> {
    // Try specialized tools first based on compositor
    if CompositorType::detect() == CompositorType::KWin {
        // KDE: use spectacle based picker
        return pick_color_kde();
    } else {
        // Try hyprpicker for wlroots compositors
        if let Ok(output) = Command::new("hyprpicker").output() {
            if output.status.success() {
                let color = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !color.is_empty() {
                    return Ok(Some(color));
                }
            }
        }
    }

    // Fallback to universal grim+slurp+imagemagick approach
    pick_color_universal_wayland(display_server)
}

fn pick_color_internal() -> Result<()> {
    let display_server = DisplayServer::detect();

    let color_opt = if matches!(display_server, DisplayServer::Wayland) {
        pick_color_with_fallbacks(&display_server)?
    } else if matches!(display_server, DisplayServer::X11) {
        // X11: xcolor
        let output = Command::new("xcolor")
            .output()
            .context("Failed to run xcolor")?;

        if !output.status.success() {
            // If user cancelled, just return Ok
            return Ok(());
        }

        let color = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if color.is_empty() {
            return Ok(());
        }
        Some(color)
    } else {
        anyhow::bail!("Unknown display server - cannot pick color");
    };

    if let Some(color) = color_opt {
        copy_to_clipboard(color.as_bytes(), &display_server)?;

        // Create preview image for notification
        let temp_dir = std::env::temp_dir();
        let icon_path = temp_dir.join(format!("color_{}.png", chrono::Local::now().timestamp()));

        // We use convert to create the preview, reusing the color hex
        let convert_status = Command::new("convert")
            .args(["-size", "45x45", &format!("xc:{}", color)])
            .arg(&icon_path)
            .status()
            .context("Failed to create preview image")?;

        if convert_status.success() {
            show_notification_with_icon(
                &format!("{} copied to clipboard", color),
                "",
                icon_path.to_str().unwrap_or(""),
            )?;

            // Clean up the temp file after a short delay
            let path_clone = icon_path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let _ = std::fs::remove_file(path_clone);
            });
        } else {
            show_notification(&format!("{} copied to clipboard", color), "")?;
        }
    }

    Ok(())
}

pub fn pick_color() -> Result<()> {
    match pick_color_internal() {
        Ok(_) => Ok(()),
        Err(e) => {
            // Show notification on failure
            let _ = show_notification("Color picker failed", &e.to_string());
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::compositor::CompositorType;
    use crate::common::display_server::DisplayServer;
    use std::env;


    #[test]
    fn test_color_picker_helper_functions_exist() {
        // We can't easily test the actual color picker functions without mocking
        // But we can verify the functions compile and are accessible
        let _unused = || {
            // These should be callable functions (commented out to avoid side effects)
            // pick_color_universal_wayland(&DisplayServer::Wayland);
        };
    }
}
