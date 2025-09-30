use anyhow::Result;
use duct::cmd;

use crate::ui::prelude::*;

use super::context::SettingsContext;

pub fn apply_clipboard_manager(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let is_running = std::process::Command::new("pgrep")
        .arg("-f")
        .arg("clipmenud")
        .output()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false);

    if enabled && !is_running {
        if let Err(err) = std::process::Command::new("clipmenud").spawn() {
            emit(
                Level::Warn,
                "settings.clipboard.spawn_failed",
                &format!(
                    "{} Failed to launch clipmenud: {err}",
                    char::from(Fa::ExclamationCircle)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud started");
        }
    } else if !enabled && is_running {
        if let Err(err) = cmd!("pkill", "-f", "clipmenud").run() {
            emit(
                Level::Warn,
                "settings.clipboard.stop_failed",
                &format!(
                    "{} Failed to stop clipmenud: {err}",
                    char::from(Fa::ExclamationCircle)
                ),
                None,
            );
        } else {
            ctx.notify("Clipboard manager", "clipmenud stopped");
        }
    }

    Ok(())
}
