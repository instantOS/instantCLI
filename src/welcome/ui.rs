//! UI components for welcome application

use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::catppuccin::{colors, format_icon_colored, hex_to_ansi_fg, select_one_with_style};
use crate::ui::prelude::*;
use anyhow::Result;

/// Welcome menu items
#[derive(Clone, Debug)]
pub enum WelcomeItem {
    InstallInstantOS,
    ConfigureNetwork,
    OpenWebsite,
    OpenSettings,
    DisableAutostart,
    Close,
}

/// Helper function to get the current autostart state
/// Returns true if autostart is enabled, false if disabled
/// If settings cannot be loaded, defaults to true (autostart enabled)
fn get_autostart_state() -> bool {
    match crate::settings::store::SettingsStore::load() {
        Ok(store) => {
            let key = crate::settings::store::BoolSettingKey::new("system.welcome_autostart", true);
            store.bool(key)
        }
        Err(_) => {
            // If we can't load settings, default to true (autostart enabled)
            true
        }
    }
}

impl FzfSelectable for WelcomeItem {
    fn fzf_display_text(&self) -> String {
        match self {
            WelcomeItem::InstallInstantOS => format!(
                "{} Install instantOS",
                format_icon_colored(NerdFont::Package, colors::GREEN)
            ),
            WelcomeItem::ConfigureNetwork => format!(
                "{} Configure Network",
                format_icon_colored(NerdFont::Wifi, colors::RED)
            ),
            WelcomeItem::OpenWebsite => format!(
                "{} Visit instantOS website",
                format_icon_colored(NerdFont::Globe, colors::BLUE)
            ),
            WelcomeItem::OpenSettings => format!(
                "{} Open Settings",
                format_icon_colored(NerdFont::Gear, colors::MAUVE)
            ),
            WelcomeItem::DisableAutostart => {
                // Check current state to show appropriate icon
                let currently_enabled = get_autostart_state();

                let (icon, color) = if currently_enabled {
                    (NerdFont::ToggleOn, colors::GREEN)
                } else {
                    (NerdFont::ToggleOff, colors::PEACH)
                };

                format!("{} Show on startup", format_icon_colored(icon, color))
            }
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
        let red = hex_to_ansi_fg(colors::RED);
        let green = hex_to_ansi_fg(colors::GREEN);

        let lines = match self {
            WelcomeItem::InstallInstantOS => vec![
                String::new(),
                format!(
                    "{green}{}  Install instantOS{reset}",
                    char::from(NerdFont::Package)
                ),
                format!("{surface}───────────────────────────────────{reset}"),
                String::new(),
                format!("{text}Launch the instantOS installation wizard{reset}"),
                format!("{text}to install instantOS on this system.{reset}"),
                String::new(),
                format!("{subtext}This will guide you through disk setup,{reset}"),
                format!("{subtext}partitioning, and system installation.{reset}"),
            ],
            WelcomeItem::ConfigureNetwork => vec![
                String::new(),
                format!("{red}{}  Network Setup{reset}", char::from(NerdFont::Wifi)),
                format!("{surface}───────────────────────────────────{reset}"),
                String::new(),
                format!("{text}No internet connection detected.{reset}"),
                format!("{text}Launch network configuration tool{reset}"),
                format!("{text}to set up your connection.{reset}"),
                String::new(),
                format!("{subtext}Opens nmtui in a new terminal.{reset}"),
            ],
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
            WelcomeItem::DisableAutostart => {
                // Check current state to show appropriate message
                let currently_enabled = get_autostart_state();

                let (icon, color, status) = if currently_enabled {
                    (NerdFont::ToggleOn, colors::GREEN, "● Enabled")
                } else {
                    (NerdFont::ToggleOff, colors::PEACH, "○ Disabled")
                };

                vec![
                    String::new(),
                    format!(
                        "{color}{}  Show on startup {status}{reset}",
                        char::from(icon),
                        color = hex_to_ansi_fg(color),
                        status = status
                    ),
                    format!("{surface}───────────────────────────────────{reset}"),
                    String::new(),
                    if currently_enabled {
                        format!("{text}The welcome app will appear{reset}")
                    } else {
                        format!("{text}The welcome app will not appear{reset}")
                    },
                    format!("{text}automatically when you log in.{reset}"),
                    String::new(),
                    format!("{subtext}You can change this setting later in{reset}"),
                    format!("{subtext}Settings > System & Updates.{reset}"),
                ]
            }
            WelcomeItem::Close => vec![
                String::new(),
                format!("{text}Exit the welcome application.{reset}"),
            ],
        };

        FzfPreview::Text(lines.join("\n"))
    }
}

pub fn run_welcome_ui(force_live: bool, debug: bool) -> Result<()> {
    if debug {
        emit(Level::Debug, "welcome.start", "Starting welcome UI", None);
    }

    // Detect live Arch ISO session
    let is_live_session = force_live || crate::common::distro::is_live_iso();

    if debug && is_live_session {
        emit(
            Level::Debug,
            "welcome.live_session",
            "Live ISO session detected",
            None,
        );
    }

    loop {
        let has_internet = crate::common::network::check_internet();

        let mut items = Vec::new();

        // Add Install instantOS option for live sessions
        if is_live_session {
            items.push(WelcomeItem::InstallInstantOS);
        }

        if !has_internet {
            items.push(WelcomeItem::ConfigureNetwork);
        }

        items.push(WelcomeItem::OpenWebsite);
        items.push(WelcomeItem::OpenSettings);
        items.push(WelcomeItem::DisableAutostart);
        items.push(WelcomeItem::Close);

        match select_one_with_style(items)? {
            Some(WelcomeItem::InstallInstantOS) => {
                if let Err(e) = install_instantos(debug) {
                    emit(
                        Level::Error,
                        "welcome.install.error",
                        &format!(
                            "{} Failed to launch installation: {}",
                            char::from(NerdFont::Warning),
                            e
                        ),
                        None,
                    );
                }
            }
            Some(WelcomeItem::ConfigureNetwork) => {
                if let Err(e) = configure_network(debug) {
                    emit(
                        Level::Error,
                        "welcome.network.error",
                        &format!(
                            "{} Failed to launch network config: {}",
                            char::from(NerdFont::Warning),
                            e
                        ),
                        None,
                    );
                }
            }
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
                // Check current state using the same helper function for consistency
                let currently_enabled = get_autostart_state();

                // Ask for confirmation
                let action_text = if currently_enabled {
                    "disable"
                } else {
                    "enable"
                };
                let confirm_message = format!(
                    "Are you sure you want to {} the welcome app autostart?\n\nThis can be changed later in Settings > System & Updates.",
                    action_text
                );

                let result = match crate::menu_utils::FzfWrapper::confirm(&confirm_message) {
                    Ok(result) => result,
                    Err(e) => {
                        eprintln!("Failed to show confirmation: {}", e);
                        continue;
                    }
                };

                match result {
                    crate::menu_utils::ConfirmResult::Yes => {
                        if let Err(e) = toggle_autostart() {
                            eprintln!("Failed to toggle autostart: {}", e);
                        } else {
                            let new_state = if currently_enabled {
                                "disabled"
                            } else {
                                "enabled"
                            };
                            println!("Welcome app autostart has been {}", new_state);
                            // Refresh the UI state by continuing the loop
                            // The next iteration will show the updated state
                            continue;
                        }
                    }
                    crate::menu_utils::ConfirmResult::No
                    | crate::menu_utils::ConfirmResult::Cancelled => {
                        // Continue the loop without making changes
                    }
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

fn configure_network(debug: bool) -> Result<()> {
    if debug {
        emit(
            Level::Debug,
            "welcome.network.config",
            "Launching nmtui in terminal",
            None,
        );
    }

    crate::common::terminal::TerminalLauncher::new("nmtui")
        .class("ins-network")
        .title("Network Setup")
        .launch()
}

fn toggle_autostart() -> Result<()> {
    use crate::settings::store::{BoolSettingKey, SettingsStore};

    let mut store = SettingsStore::load()?;
    let key = BoolSettingKey::new("system.welcome_autostart", true);
    let current_value = store.bool(key);
    store.set_bool(key, !current_value);
    store.save()?;

    Ok(())
}

fn install_instantos(debug: bool) -> Result<()> {
    if debug {
        emit(
            Level::Debug,
            "welcome.install.launch",
            "Launching instantOS installation in terminal",
            None,
        );
    }

    crate::common::terminal::TerminalLauncher::new("ins")
        .arg("arch")
        .arg("ask")
        .class("ins-install")
        .title("instantOS Installation")
        .launch()
}
