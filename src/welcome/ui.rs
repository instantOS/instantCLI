//! UI components for welcome application

use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::settings::context::{
    colors, format_icon_colored, hex_to_ansi_fg, select_one_with_style,
};
use crate::ui::prelude::*;
use anyhow::Result;

/// Welcome menu items
#[derive(Clone, Debug)]
pub enum WelcomeItem {
    OpenWebsite,
    OpenSettings,
    DisableAutostart,
    Close,
}

impl FzfSelectable for WelcomeItem {
    fn fzf_display_text(&self) -> String {
        match self {
            WelcomeItem::OpenWebsite => format!(
                "{} Visit instantOS website",
                format_icon_colored(NerdFont::Globe, colors::BLUE)
            ),
            WelcomeItem::OpenSettings => format!(
                "{} Open Settings",
                format_icon_colored(NerdFont::Gear, colors::MAUVE)
            ),
            WelcomeItem::DisableAutostart => format!(
                "{} Disable welcome app on startup",
                format_icon_colored(NerdFont::PowerOff, colors::PEACH)
            ),
            WelcomeItem::Close => format!(
                "{} Close",
                format_icon_colored(NerdFont::Cross, colors::OVERLAY1)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        let reset = "\x1b[0m";
        let text = hex_to_ansi_fg(colors::TEXT);
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let surface = hex_to_ansi_fg(colors::SURFACE1);
        let blue = hex_to_ansi_fg(colors::BLUE);
        let mauve = hex_to_ansi_fg(colors::MAUVE);
        let peach = hex_to_ansi_fg(colors::PEACH);

        let lines = match self {
            WelcomeItem::OpenWebsite => vec![
                String::new(),
                format!(
                    "{blue}{}  instantOS Website{reset}",
                    char::from(NerdFont::Globe)
                ),
                format!("{surface}───────────────────────────────────{reset}"),
                String::new(),
                format!("{text}Visit instantos.io to learn more about{reset}"),
                format!("{text}instantOS and access documentation,{reset}"),
                format!("{text}community forums, and downloads.{reset}"),
                String::new(),
                format!("{subtext}Opens in your default web browser.{reset}"),
            ],
            WelcomeItem::OpenSettings => vec![
                String::new(),
                format!("{mauve}{}  Settings{reset}", char::from(NerdFont::Gear)),
                format!("{surface}───────────────────────────────────{reset}"),
                String::new(),
                format!("{text}Access the instantOS settings manager{reset}"),
                format!("{text}to customize your desktop environment.{reset}"),
                String::new(),
                format!("{subtext}Configure appearance, applications,{reset}"),
                format!("{subtext}keyboard, mouse, and more.{reset}"),
            ],
            WelcomeItem::DisableAutostart => vec![
                String::new(),
                format!(
                    "{peach}{}  Disable Autostart{reset}",
                    char::from(NerdFont::PowerOff)
                ),
                format!("{surface}───────────────────────────────────{reset}"),
                String::new(),
                format!("{text}Prevent the welcome app from{reset}"),
                format!("{text}appearing automatically on startup.{reset}"),
                String::new(),
                format!("{subtext}You can re-enable this later in{reset}"),
                format!("{subtext}Settings > System & Updates.{reset}"),
            ],
            WelcomeItem::Close => vec![
                String::new(),
                format!("{text}Exit the welcome application.{reset}"),
            ],
        };

        FzfPreview::Text(lines.join("\n"))
    }
}

pub fn run_welcome_ui(debug: bool) -> Result<()> {
    if debug {
        emit(Level::Debug, "welcome.start", "Starting welcome UI", None);
    }

    loop {
        let items = vec![
            WelcomeItem::OpenWebsite,
            WelcomeItem::OpenSettings,
            WelcomeItem::DisableAutostart,
            WelcomeItem::Close,
        ];

        match select_one_with_style(items)? {
            Some(WelcomeItem::OpenWebsite) => {
                if let Err(e) = open_website(debug) {
                    emit(
                        Level::Error,
                        "welcome.website.error",
                        &format!(
                            "{} Failed to open website: {}",
                            char::from(NerdFont::Warning),
                            e
                        ),
                        None,
                    );
                }
            }
            Some(WelcomeItem::OpenSettings) => {
                if let Err(e) = open_settings(debug) {
                    emit(
                        Level::Error,
                        "welcome.settings.error",
                        &format!(
                            "{} Failed to open settings: {}",
                            char::from(NerdFont::Warning),
                            e
                        ),
                        None,
                    );
                }
            }
            Some(WelcomeItem::DisableAutostart) => {
                if let Err(e) = disable_autostart(debug) {
                    emit(
                        Level::Error,
                        "welcome.disable.error",
                        &format!(
                            "{} Failed to disable autostart: {}",
                            char::from(NerdFont::Warning),
                            e
                        ),
                        None,
                    );
                } else {
                    emit(
                        Level::Success,
                        "welcome.disabled",
                        &format!(
                            "{} Welcome app autostart has been disabled",
                            char::from(NerdFont::Check)
                        ),
                        None,
                    );
                    // Exit after disabling
                    break;
                }
            }
            Some(WelcomeItem::Close) | None => {
                if debug {
                    emit(Level::Debug, "welcome.close", "Closing welcome UI", None);
                }
                break;
            }
        }
    }

    Ok(())
}

fn open_website(debug: bool) -> Result<()> {
    use std::process::Command;

    if debug {
        emit(
            Level::Debug,
            "welcome.website.open",
            "Opening instantos.io",
            None,
        );
    }

    Command::new("xdg-open")
        .arg("https://instantos.io")
        .spawn()
        .map(|_| ())
        .or_else(|_| {
            // Fallback to other browsers if xdg-open fails
            Command::new("firefox")
                .arg("https://instantos.io")
                .spawn()
                .map(|_| ())
        })
        .or_else(|_| {
            Command::new("chromium")
                .arg("https://instantos.io")
                .spawn()
                .map(|_| ())
        })?;

    Ok(())
}

fn open_settings(debug: bool) -> Result<()> {
    use std::process::Command;

    if debug {
        emit(
            Level::Debug,
            "welcome.settings.open",
            "Opening settings with --gui",
            None,
        );
    }

    let current_exe = std::env::current_exe()?;

    Command::new(&current_exe)
        .arg("settings")
        .arg("--gui")
        .spawn()?;

    Ok(())
}

fn disable_autostart(debug: bool) -> Result<()> {
    use crate::settings::store::{BoolSettingKey, SettingsStore};

    if debug {
        emit(
            Level::Debug,
            "welcome.autostart.disable",
            "Disabling welcome autostart setting",
            None,
        );
    }

    let mut store = SettingsStore::load()?;
    let key = BoolSettingKey::new("system.welcome_autostart", true);
    store.set_bool(key, false);
    store.save()?;

    Ok(())
}
