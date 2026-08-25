use anyhow::{Context, Result};

pub fn caffeine() -> Result<()> {
    use crate::assist::utils;
    use crate::common::display_server::DisplayServer;

    match DisplayServer::detect() {
        DisplayServer::Wayland | DisplayServer::X11 => {
            let command = "clear && figlet -w 120 'Caffeine' && echo '' && echo 'Caffeine running - press Ctrl+C to quit' && systemd-inhibit --what=idle --who=Caffeine --why=Caffeine --mode=block sleep inf";
            utils::launch_in_terminal(command)?;
            Ok(())
        }
        DisplayServer::Unknown => {
            anyhow::bail!("Unknown display server. Caffeine requires a running display server.");
        }
    }
}

pub fn volume() -> Result<()> {
    crate::assist::utils::menu_command(&["slide", "--preset", "audio"])
}

pub fn brightness() -> Result<()> {
    crate::assist::utils::menu_command(&["slide", "--preset", "brightness"])
}

pub fn theme_settings() -> Result<()> {
    crate::settings::apply::run_nonpersistent_apply(false, false)
}

/// Direct volume control: +, -, mute, or absolute percentage
pub fn volume_direct(action: &str) -> Result<()> {
    use std::process::Command;

    match action {
        "+" => {
            let _ = Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", "5%+"])
                .status();
        }
        "-" => {
            let _ = Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"])
                .status();
        }
        "mute" => {
            let _ = Command::new("wpctl")
                .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
                .status();
        }
        other => {
            let val: i64 = other
                .parse()
                .context("Expected +, -, mute, or a number (0-100)")?;
            let _ = Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{}%", val)])
                .status();
        }
    }

    // Show notification with current volume level
    if let Ok(output) = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(vol) = stdout.split_whitespace().find_map(|t| {
            t.trim_matches(|c: char| matches!(c, '[' | ']' | ',' | ':'))
                .parse::<f64>()
                .ok()
        }) {
            let percent = (vol * 100.0).round() as i64;
            let _ = Command::new("dunstify")
                .args([
                    "--appname",
                    "instantCLI",
                    "--transient",
                    "-h",
                    "string:x-dunst-stack-tag:instantcli-volume",
                    "-h",
                    &format!("int:value:{}", percent),
                    "-i",
                    "audio-volume-medium-symbolic",
                    &format!("Volume [{}%]", percent),
                ])
                .spawn();
        }
    }

    Ok(())
}

/// Toggle the microphone mute state.
///
/// Toggles the first ALSA capture switch found on any sound card; the
/// hardware mic-mute LED follows automatically (audio-micmute LED trigger).
/// The PipeWire default source mute is synced afterwards so desktop mixers
/// show the same state.
pub fn mic_mute() -> Result<()> {
    use std::process::Command;

    for card in 0..10 {
        let Ok(out) = Command::new("amixer")
            .args(["-c", &card.to_string(), "sget", "Capture"])
            .output()
        else {
            continue;
        };
        let info = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || !info.contains("cswitch") {
            continue;
        }

        Command::new("amixer")
            .args(["-c", &card.to_string(), "sset", "Capture", "toggle"])
            .status()
            .context("Failed to toggle the ALSA capture switch")?;

        let muted = if let Ok(after) = Command::new("amixer")
            .args(["-c", &card.to_string(), "sget", "Capture"])
            .output()
        {
            !String::from_utf8_lossy(&after.stdout).contains("[on]")
        } else {
            false
        };
        let _ = Command::new("wpctl")
            .args([
                "set-mute",
                "@DEFAULT_AUDIO_SOURCE@",
                if muted { "1" } else { "0" },
            ])
            .status();
        notify_mic_mute(muted);
        return Ok(());
    }

    // No ALSA capture switch found: fall back to the PipeWire source mute.
    let _ = Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"])
        .status();
    Ok(())
}

fn notify_mic_mute(muted: bool) {
    use std::process::Command;

    let (icon, text) = if muted {
        ("microphone-sensitivity-muted-symbolic", "Microphone muted")
    } else {
        ("microphone-sensitivity-high-symbolic", "Microphone unmuted")
    };
    let _ = Command::new("dunstify")
        .args([
            "--appname",
            "instantCLI",
            "--transient",
            "-h",
            "string:x-dunst-stack-tag:instantcli-micmute",
            "-i",
            icon,
            text,
        ])
        .spawn();
}

/// Direct brightness control: + or -, or absolute percentage
pub fn brightness_direct(action: &str) -> Result<()> {
    use std::process::Command;

    match action {
        "+" => {
            let _ = Command::new("brightnessctl")
                .args(["--quiet", "set", "5%+"])
                .status();
        }
        "-" => {
            let _ = Command::new("brightnessctl")
                .args(["--quiet", "set", "5%-"])
                .status();
        }
        other => {
            let val: i64 = other
                .parse()
                .context("Expected +, -, or a number (0-100)")?;
            let _ = Command::new("brightnessctl")
                .args(["--quiet", "set", &format!("{}%", val)])
                .status();
        }
    }

    // Show notification with current brightness level
    let current = Command::new("brightnessctl").arg("get").output().ok();
    let max = Command::new("brightnessctl").arg("max").output().ok();
    if let (Some(cur), Some(mx)) = (current, max)
        && cur.status.success()
        && mx.status.success()
    {
        let cur_val: f64 = String::from_utf8_lossy(&cur.stdout)
            .trim()
            .parse()
            .unwrap_or(0.0);
        let max_val: f64 = String::from_utf8_lossy(&mx.stdout)
            .trim()
            .parse()
            .unwrap_or(1.0);
        if max_val > 0.0 {
            let percent = (cur_val / max_val * 100.0).round() as i64;
            let _ = Command::new("dunstify")
                .args([
                    "--appname",
                    "instantCLI",
                    "--transient",
                    "-h",
                    "string:x-dunst-stack-tag:instantcli-brightness",
                    "-h",
                    &format!("int:value:{}", percent),
                    "-i",
                    "display-brightness-medium-symbolic",
                    &format!("Brightness [{}%]", percent),
                ])
                .spawn();
        }
    }

    Ok(())
}
