//! Swap Escape and Caps Lock setting

use anyhow::{Context, Result};
use regex;

use crate::common::compositor::{CompositorType, sway};
use crate::menu_utils::FzfWrapper;
use crate::settings::context::SettingsContext;
use crate::settings::setting::{Setting, SettingMetadata, SettingType};
use crate::settings::store::BoolSettingKey;
use crate::ui::prelude::*;

/// Swap Escape and Caps Lock keys
pub struct SwapEscape;

impl SwapEscape {
    const KEY: BoolSettingKey = BoolSettingKey::new("desktop.swap_escape", false);
}

/// Apply swap escape setting with configurable verbosity
fn apply_swap_escape_setting_impl(
    ctx: &mut SettingsContext,
    enabled: bool,
    verbose: bool,
) -> Result<()> {
    let compositor = CompositorType::detect();
    let is_sway = matches!(compositor, CompositorType::Sway);
    let is_gnome = matches!(compositor, CompositorType::Gnome);
    let is_kwin = matches!(compositor, CompositorType::KWin);
    let is_x11 = compositor.is_x11();

    if !is_sway && !is_gnome && !is_kwin && !is_x11 {
        if verbose {
            let message = format!(
                "Swap Escape/Caps Lock configuration is not yet supported on {}. Setting saved but not applied.",
                compositor.name()
            );
            let _ = FzfWrapper::message_dialog(&message);
        }
        return Ok(());
    }

    if is_sway {
        let xkb_options = if enabled { "caps:swapescape" } else { "" };
        let cmd = format!("input type:keyboard xkb_options {}", xkb_options);
        match sway::swaymsg(&cmd) {
            Ok(_) => {
                if verbose {
                    ctx.notify(
                        "Swap Escape/Caps Lock",
                        if enabled {
                            "Escape and Caps Lock keys swapped"
                        } else {
                            "Escape and Caps Lock keys restored to normal"
                        },
                    );
                }
            }
            Err(e) => {
                if verbose {
                    let message = format!("Failed to apply in Sway: {e}");
                    let _ = FzfWrapper::message_dialog(&message);
                }
            }
        }
    } else if is_gnome {
        let result = if enabled {
            std::process::Command::new("gsettings")
                .args([
                    "set",
                    "org.gnome.desktop.input-sources",
                    "xkb-options",
                    "['caps:swapescape']",
                ])
                .status()
        } else {
            std::process::Command::new("gsettings")
                .args(["reset", "org.gnome.desktop.input-sources", "xkb-options"])
                .status()
        };

        match result {
            Ok(status) if status.success() => {
                if verbose {
                    ctx.notify(
                        "Swap Escape/Caps Lock",
                        if enabled {
                            "Escape and Caps Lock keys swapped"
                        } else {
                            "Escape and Caps Lock keys restored to normal"
                        },
                    );
                }
            }
            Ok(_) => {
                if verbose {
                    let message = "gsettings command failed to apply the setting.";
                    let _ = FzfWrapper::message_dialog(message);
                }
            }
            Err(e) => {
                if verbose {
                    let message = format!("Failed to execute gsettings: {e}");
                    let _ = FzfWrapper::message_dialog(&message);
                }
            }
        }
    } else if is_kwin {
        // KWin/KDE keyboard configuration through kxkbrc
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let kxkbrc_path = format!("{}/.config/kxkbrc", home_dir);

        // Read existing configuration or create new one
        let mut config_content = std::fs::read_to_string(&kxkbrc_path)
            .unwrap_or_else(|_| "[Layout]\nUse=true\n".to_string());

        // Update or add the Options line
        let options_line = if enabled {
            "Options=grp:alt_shift_toggle,caps:swapescape"
        } else {
            "Options=grp:alt_shift_toggle"
        };

        // Replace existing Options line or add it
        if config_content.contains("Options=") {
            config_content = regex::Regex::new(r"Options=.*")
                .unwrap()
                .replace(&config_content, options_line)
                .to_string();
        } else {
            config_content.push_str(&format!("\n{}\n", options_line));
        }

        // Write configuration back
        if verbose {
            std::fs::write(&kxkbrc_path, config_content).with_context(|| {
                format!(
                    "Failed to write KDE keyboard configuration to {}",
                    kxkbrc_path
                )
            })?;
        } else {
            let _ = std::fs::write(&kxkbrc_path, config_content);
        }

        // Apply the configuration by restarting KDE keyboard daemon
        if verbose {
            // Try multiple methods to reload the configuration
            let mut applied_successfully = false;

            // Method 1: Try to restart kxkb daemon (KDE 5)
            if let Ok(_) = std::process::Command::new("killall")
                .args(["-USR1", "kxkb"])
                .status()
            {
                applied_successfully = true;
            }

            // Method 2: Try kwriteconfig6 to force reapply (KDE 6)
            if !applied_successfully
                && let Ok(_) = std::process::Command::new("kwriteconfig6")
                    .args([
                        "--file", "kxkbrc", "--group", "Layout", "--key", "Use", "--type", "bool",
                        "true",
                    ])
                    .status()
            {
                applied_successfully = true;
            }

            // Method 3: Try kwriteconfig5 as fallback (KDE 5)
            if !applied_successfully
                && let Ok(_) = std::process::Command::new("kwriteconfig5")
                    .args([
                        "--file", "kxkbrc", "--group", "Layout", "--key", "Use", "--type", "bool",
                        "true",
                    ])
                    .status()
            {
                applied_successfully = true;
            }

            // Method 4: Try using qdbus to trigger layout reload
            if !applied_successfully
                && std::process::Command::new("qdbus")
                    .args([
                        "org.kde.keyboard",
                        "/Layouts",
                        "org.kde.KeyboardLayouts.switchToNextLayout",
                    ])
                    .status()
                    .is_ok_and(|s| s.success())
            {
                // Switch back to original layout
                let _ = std::process::Command::new("qdbus")
                    .args([
                        "org.kde.keyboard",
                        "/Layouts",
                        "org.kde.KeyboardLayouts.switchToPreviousLayout",
                    ])
                    .status();
                applied_successfully = true;
            }

            if applied_successfully {
                ctx.notify(
                    "Swap Escape/Caps Lock",
                    if enabled {
                        "Escape and Caps Lock keys swapped"
                    } else {
                        "Escape and Caps Lock keys restored to normal"
                    },
                );
            } else {
                let message = "KDE configuration updated, but requires restart or manual reapply.\n\nSystem Settings → Input Devices → Keyboard → Advanced → Caps Lock behavior\n\nOr restart the keyboard daemon by logging out and back in.";
                let _ = FzfWrapper::message_dialog(message);
            }
        }
    } else {
        let result = if enabled {
            std::process::Command::new("setxkbmap")
                .args(["-option", "caps:swapescape"])
                .status()
        } else {
            std::process::Command::new("setxkbmap")
                .args(["-option", ""])
                .status()
        };

        match result {
            Ok(status) if status.success() => {
                if verbose {
                    ctx.notify(
                        "Swap Escape/Caps Lock",
                        if enabled {
                            "Escape and Caps Lock keys swapped"
                        } else {
                            "Escape and Caps Lock keys restored to normal"
                        },
                    );
                }
            }
            Ok(_) => {
                if verbose {
                    let message = "setxkbmap command failed to apply the setting.";
                    let _ = FzfWrapper::message_dialog(message);
                }
            }
            Err(e) => {
                if verbose {
                    let message = format!("Failed to execute setxkbmap: {e}");
                    let _ = FzfWrapper::message_dialog(&message);
                }
            }
        }
    }

    Ok(())
}

/// Apply swap escape setting (shared by both apply and restore)
fn apply_swap_escape_setting(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    apply_swap_escape_setting_impl(ctx, enabled, true)
}

/// Apply swap escape setting silently (for restore operations)
fn apply_swap_escape_setting_silent(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    apply_swap_escape_setting_impl(ctx, enabled, false)
}

impl Setting for SwapEscape {
    fn metadata(&self) -> SettingMetadata {
        SettingMetadata::builder()
            .id("desktop.swap_escape")
            .title("Swap Escape and Caps Lock")
            .icon(NerdFont::Keyboard)
            .summary("Swap the Escape and Caps Lock keys.\n\nWhen enabled, pressing Caps Lock will produce Escape and vice versa.\n\nSupports Sway, GNOME, KWin/KDE, and X11 window managers.")
            .requires_reapply(true)
            .build()
    }

    fn setting_type(&self) -> SettingType {
        SettingType::Toggle { key: Self::KEY }
    }

    fn apply(&self, ctx: &mut SettingsContext) -> Result<()> {
        let current = ctx.bool(Self::KEY);
        let enabled = !current;
        ctx.set_bool(Self::KEY, enabled);
        self.apply_value(ctx, enabled)
    }

    fn apply_value(&self, ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
        apply_swap_escape_setting(ctx, enabled)
    }

    fn restore(&self, ctx: &mut SettingsContext) -> Option<Result<()>> {
        let enabled = ctx.bool(Self::KEY);
        if !enabled {
            return None;
        }
        Some(apply_swap_escape_setting_silent(ctx, enabled))
    }
}
