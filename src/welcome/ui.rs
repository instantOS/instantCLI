//! UI components for welcome application

use crate::menu_utils::select_one_with_style_at;
use crate::menu_utils::{FzfPreview, FzfSelectable, MenuCursor};
use crate::ui::catppuccin::{colors, format_icon_colored};
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
        use crate::ui::preview::PreviewBuilder;

        match self {
            WelcomeItem::InstallInstantOS => PreviewBuilder::new()
                .line(colors::GREEN, Some(NerdFont::Package), "Install instantOS")
                .separator()
                .blank()
                .text("Launch the instantOS installation wizard")
                .text("to install instantOS on this system.")
                .blank()
                .subtext("This will guide you through disk setup,")
                .subtext("partitioning, and system installation.")
                .build(),
            WelcomeItem::ConfigureNetwork => PreviewBuilder::new()
                .line(colors::RED, Some(NerdFont::Wifi), "Network Setup")
                .separator()
                .blank()
                .text("No internet connection detected.")
                .text("Launch network configuration tool")
                .text("to set up your connection.")
                .blank()
                .subtext("Opens nmtui in a new terminal.")
                .build(),
            WelcomeItem::OpenWebsite => PreviewBuilder::new()
                .line(colors::BLUE, Some(NerdFont::Globe), "instantOS Website")
                .separator()
                .blank()
                .text("Visit instantos.io to learn more about")
                .text("instantOS and access documentation,")
                .text("community forums, and downloads.")
                .blank()
                .subtext("Opens in your default web browser.")
                .build(),
            WelcomeItem::OpenSettings => PreviewBuilder::new()
                .line(colors::MAUVE, Some(NerdFont::Gear), "Settings")
                .separator()
                .blank()
                .text("Access the instantOS settings manager")
                .text("to customize your desktop environment.")
                .blank()
                .subtext("Configure appearance, applications,")
                .subtext("keyboard, mouse, and more.")
                .build(),
            WelcomeItem::DisableAutostart => {
                let currently_enabled = get_autostart_state();

                let (icon, color, status) = if currently_enabled {
                    (NerdFont::ToggleOn, colors::GREEN, "● Enabled")
                } else {
                    (NerdFont::ToggleOff, colors::PEACH, "○ Disabled")
                };

                let description = if currently_enabled {
                    "The welcome app will appear"
                } else {
                    "The welcome app will not appear"
                };

                PreviewBuilder::new()
                    .line(color, Some(icon), &format!("Show on startup {}", status))
                    .separator()
                    .blank()
                    .text(description)
                    .text("automatically when you log in.")
                    .blank()
                    .subtext("You can change this setting later in")
                    .subtext("Settings > System & Updates.")
                    .build()
            }
            WelcomeItem::Close => PreviewBuilder::new()
                .text("Exit the welcome application.")
                .build(),
        }
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

    let mut cursor = MenuCursor::new();

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

        let initial_cursor = cursor.initial_index(&items);
        match select_one_with_style_at(items.clone(), initial_cursor)? {
            Some(WelcomeItem::InstallInstantOS) => {
                cursor.update(&WelcomeItem::InstallInstantOS, &items);
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
                cursor.update(&WelcomeItem::ConfigureNetwork, &items);
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
                cursor.update(&WelcomeItem::OpenWebsite, &items);
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
                cursor.update(&WelcomeItem::OpenSettings, &items);
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
                cursor.update(&WelcomeItem::DisableAutostart, &items);
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
                if let Some(selected) = items.last() {
                    cursor.update(selected, &items);
                }
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
