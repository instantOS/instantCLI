use crate::common::compositor::CompositorType;
use crate::common::paths;
use crate::common::systemd;
use crate::setup::{SetupCommands, handle_setup_command};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const NOTIFICATIONS_BUS_NAME: &str = "org.freedesktop.Notifications";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutostartConfig {
    #[serde(default)]
    pub disabled: bool,
}

pub fn load_config() -> Result<AutostartConfig> {
    let config_dir = paths::instant_config_dir()?;
    let config_path = config_dir.join("autostart.toml");

    if !config_path.exists() {
        return Ok(AutostartConfig::default());
    }

    let content = fs::read_to_string(&config_path).context("Failed to read autostart config")?;

    toml::from_str(&content).context("Failed to parse autostart config")
}

/// Exclusive lock ensuring only one autostart runs at a time.
///
/// The lock is an advisory `flock` held on a file, so the kernel releases it
/// automatically when the process dies. A PID file cannot do this: a hung
/// autostart from a previous session stays alive (reparented to init) and its
/// recorded PID keeps looking valid, which used to silently skip autostart —
/// including the wallpaper — in every subsequent session.
pub struct AutostartGuard {
    /// Held for the lifetime of the guard; `Flock` unlocks on drop.
    _file: nix::fcntl::Flock<fs::File>,
}

impl AutostartGuard {
    /// Try to acquire the autostart lock. Returns `Ok(None)` when another
    /// autostart process is currently holding it.
    pub fn acquire() -> Result<Option<AutostartGuard>> {
        Self::acquire_at(&lock_file_path())
    }

    fn acquire_at(path: &std::path::Path) -> Result<Option<AutostartGuard>> {
        // Never truncate on open: the file may belong to the running
        // autostart whose lock we are about to (fail to) take.
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .with_context(|| format!("Failed to open {}", path.display()))?;

        let mut file = match nix::fcntl::Flock::lock(
            file,
            nix::fcntl::FlockArg::LockExclusiveNonblock,
        ) {
            Ok(locked) => locked,
            Err((_file, nix::errno::Errno::EWOULDBLOCK)) => return Ok(None),
            Err((_file, err)) => {
                return Err(err).context("Failed to lock the autostart lock file");
            }
        };

        // Record the PID for diagnostics only; the flock itself is the lock.
        use std::io::{Seek, SeekFrom, Write};
        file.seek(SeekFrom::Start(0))
            .context("Failed to seek the autostart lock file")?;
        let pid = std::process::id().to_string();
        file.set_len(pid.len() as u64)
            .context("Failed to truncate the autostart lock file")?;
        file.write_all(pid.as_bytes())
            .context("Failed to write the autostart lock file")?;

        Ok(Some(AutostartGuard { _file: file }))
    }
}

fn lock_file_path() -> PathBuf {
    // XDG_RUNTIME_DIR is per-user and wiped at logout; fall back to /tmp.
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("instant_autostart.lock")
}

pub async fn run(debug: bool) -> Result<()> {
    let config = load_config()?;

    if config.disabled {
        if debug {
            println!("Autostart is disabled in config");
        }
        return Ok(());
    }

    let Some(_autostart_guard) = AutostartGuard::acquire()? else {
        if debug {
            println!("Autostart is already running");
        }
        return Ok(());
    };

    if crate::common::distro::is_live_iso() {
        if debug {
            println!("Applying live-session setup");
        }
        match std::process::Command::new("liveautostart").status() {
            Ok(status) if !status.success() => {
                eprintln!("Live-session setup exited with status: {}", status);
            }
            Err(e) => eprintln!("Failed to run live-session setup: {}", e),
            Ok(_) => {}
        }

        if let Err(e) = std::process::Command::new("installapplet").spawn() {
            eprintln!("Failed to launch installer applet: {}", e);
        }
    }

    if which::which("nvidia-settings").is_ok() {
        if debug {
            println!("Found nvidia-settings, loading settings");
        }
        if let Err(e) = std::process::Command::new("nvidia-settings")
            .arg("-l")
            .status()
            && debug
        {
            eprintln!("Failed to run nvidia-settings: {}", e);
        }
    }

    ensure_notification_daemon(debug).await;

    if debug {
        println!("Applying settings");
    }
    if let Err(e) = crate::settings::commands::dispatch_settings_command(
        debug,
        false,
        Some(crate::settings::commands::SettingsCommands::Apply),
        None,
    ) && debug
    {
        eprintln!("Failed to apply settings: {}", e);
    }

    // Apply wallpaper before anything that can block on the network. It only
    // depends on the local settings store, so a slow or unreachable dotfiles
    // remote must never delay (or, via the autostart lock, skip) it.
    if debug {
        println!("Applying wallpaper");
    }
    if let Err(e) = crate::wallpaper::commands::apply_configured_wallpaper().await
        && debug
    {
        eprintln!("Failed to apply wallpaper: {}", e);
    }

    // Run sway/i3/niri setup based on detected compositor
    match CompositorType::detect() {
        CompositorType::Sway => {
            if debug {
                println!("Running sway setup");
            }
            if let Err(e) = handle_setup_command(SetupCommands::Sway)
                && debug
            {
                eprintln!("Sway setup failed: {}", e);
            }
        }
        CompositorType::I3 => {
            if debug {
                println!("Running i3 setup");
            }
            if let Err(e) = handle_setup_command(SetupCommands::I3)
                && debug
            {
                eprintln!("i3 setup failed: {}", e);
            }
        }
        CompositorType::Niri => {
            if debug {
                println!("Running niri setup");
            }
            if let Err(e) = handle_setup_command(SetupCommands::Niri)
                && debug
            {
                eprintln!("niri setup failed: {}", e);
            }
        }
        _ => {
            if debug {
                println!("Not running Sway, i3, or niri, skipping window manager setup");
            }
        }
    }

    // Refresh dotfiles in a detached background process. The dot update is a
    // network operation with unbounded duration (a stalled git remote once
    // wedged autostart here for good), and nothing below depends on it.
    if crate::common::network::check_internet() {
        if debug {
            println!("Internet connection detected, starting background dot update");
        }
        if let Err(e) = spawn_detached_dot_update()
            && debug
        {
            eprintln!("Failed to start background dot update: {e}");
        }
    } else if debug {
        println!("No internet connection detected");
    }

    // Start polkit agent if needed
    ensure_polkit_agent(debug).await;

    // Launch welcome app if enabled
    if should_show_welcome() {
        if debug {
            println!("Launching welcome app");
        }
        if let Err(e) = crate::welcome::commands::handle_welcome_command(&None, true, false, debug)
            && debug
        {
            eprintln!("Failed to launch welcome app: {}", e);
        }
    } else if debug {
        println!("Welcome app autostart is disabled");
    }

    Ok(())
}

/// Start `ins dot update` as a fully detached background process.
///
/// stdio is discarded and the process gets its own process group, so it
/// cannot keep the autostart process (and therefore the autostart lock)
/// alive, and it is never signaled together with the session.
fn spawn_detached_dot_update() -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    // Re-exec through current_exe rather than PATH lookup: display-manager
    // sessions often have a minimal PATH, and this keeps development builds
    // self-consistent.
    let exe = std::env::current_exe().context("resolving the current executable")?;
    Command::new(exe)
        .args(["dot", "update"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn background dot update")?;
    Ok(())
}

async fn notification_daemon_running() -> Result<bool> {
    let connection = zbus::Connection::session()
        .await
        .context("connecting to the session D-Bus")?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(NOTIFICATIONS_BUS_NAME,),
        )
        .await
        .context("checking for a notification daemon")?;
    reply
        .body()
        .deserialize()
        .context("reading notification daemon status")
}

async fn ensure_notification_daemon(debug: bool) {
    use crate::common::display_server::DisplayServer;

    match notification_daemon_running().await {
        Ok(true) => {
            if debug {
                println!("A notification daemon already owns {NOTIFICATIONS_BUS_NAME}");
            }
            return;
        }
        Err(e) => {
            // If ownership cannot be checked, starting another implementation
            // risks replacing or conflicting with the user's daemon.
            if debug {
                eprintln!("Could not inspect the notification D-Bus service: {e}");
            }
            return;
        }
        Ok(false) => {}
    }

    let Some(service) = notification_service_for(&DisplayServer::detect()) else {
        if debug {
            println!("No graphical display server detected; skipping notification daemon");
        }
        return;
    };

    if let Err(e) = systemd::ensure_graphical_session_target() {
        if debug {
            eprintln!("Could not activate graphical-session.target: {e}");
        }
        return;
    }

    let manager = systemd::SystemdManager::user();
    if !manager.service_exists(service) {
        if debug {
            eprintln!("Preferred notification service is not installed: {service}");
        }
        return;
    }

    if let Err(e) = manager.start(service) {
        eprintln!("Failed to start notification daemon {service}: {e}");
    } else if debug {
        println!("Started notification daemon through {service}");
    }
}

fn notification_service_for(
    display_server: &crate::common::display_server::DisplayServer,
) -> Option<&'static str> {
    use crate::common::display_server::DisplayServer;

    match display_server {
        DisplayServer::Wayland => Some("mako.service"),
        DisplayServer::X11 => Some("dunst.service"),
        DisplayServer::Unknown => None,
    }
}

async fn ensure_polkit_agent(debug: bool) {
    use crate::common::display_server::DisplayServer;

    // Skip if not a desktop session
    if !DisplayServer::detect().is_desktop_session() {
        if debug {
            println!("Not running in a desktop session, skipping polkit agent setup");
        }
        return;
    }

    // Check if polkit agent is already running
    if crate::doctor::checks::security::is_polkit_agent_running().await {
        if debug {
            println!("Polkit authentication agent is already running");
        }
        return;
    }

    if debug {
        println!("No polkit authentication agent found, attempting to start one");
    }

    // Try to start hyprpolkitagent via systemd user service if available
    let systemd_manager = systemd::SystemdManager::user();
    if systemd_manager.service_exists("hyprpolkitagent.service") {
        if debug {
            println!("hyprpolkitagent.service found, attempting to start systemd service");
        }

        match systemd_manager.start("hyprpolkitagent.service") {
            Ok(()) => {
                if debug {
                    println!("Successfully started hyprpolkitagent service");
                }
                return;
            }
            Err(e) => {
                if debug {
                    eprintln!("Failed to start hyprpolkitagent service: {}", e);
                }
            }
        }
    }

    // Fallback: try to start hyprpolkitagent via PATH if available
    if which::which("hyprpolkitagent").is_ok() {
        if debug {
            println!("hyprpolkitagent found in PATH, attempting to start in background");
        }

        match std::process::Command::new("hyprpolkitagent").spawn() {
            Ok(child) => {
                std::mem::forget(child);
                if debug {
                    println!("Successfully started hyprpolkitagent in background");
                }
                return;
            }
            Err(e) => {
                if debug {
                    eprintln!("Failed to start hyprpolkitagent: {}", e);
                }
            }
        }
    }

    // Fallback: try to start lxpolkit in background if installed
    if which::which("lxpolkit").is_ok() {
        if debug {
            println!("hyprpolkitagent not available, trying lxpolkit as fallback");
        }

        match std::process::Command::new("lxpolkit").spawn() {
            Ok(child) => {
                std::mem::forget(child);
                if debug {
                    println!("Successfully started lxpolkit in background");
                }
                return;
            }
            Err(e) => {
                if debug {
                    eprintln!("Failed to start lxpolkit: {}", e);
                }
            }
        }
    }

    if debug {
        println!(
            "No polkit agent could be started. Neither hyprpolkitagent nor lxpolkit are available."
        );
    }
}

fn should_show_welcome() -> bool {
    use crate::settings::store::{BoolSettingKey, SettingsStore};

    // Try to load settings and check if welcome autostart is enabled
    match SettingsStore::load() {
        Ok(store) => {
            let key = BoolSettingKey::new("system.welcome_autostart", true);
            store.bool(key)
        }
        Err(_) => {
            // If we can't load settings, default to true (show welcome on first boot)
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::notification_service_for;
    use super::AutostartGuard;
    use crate::common::display_server::DisplayServer;

    #[test]
    fn autostart_lock_excludes_second_holder_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instant_autostart.lock");

        let guard = AutostartGuard::acquire_at(&path)
            .unwrap()
            .expect("first acquire must succeed");
        assert!(
            AutostartGuard::acquire_at(&path).unwrap().is_none(),
            "second acquire while held must be refused"
        );

        // Releasing (or the process dying) must free the lock for the next
        // session; a stale PID file would have kept refusing here.
        drop(guard);
        assert!(
            AutostartGuard::acquire_at(&path).unwrap().is_some(),
            "acquire must succeed again after release"
        );
    }

    #[test]
    fn chooses_notification_daemon_for_display_server() {
        assert_eq!(
            notification_service_for(&DisplayServer::Wayland),
            Some("mako.service")
        );
        assert_eq!(
            notification_service_for(&DisplayServer::X11),
            Some("dunst.service")
        );
        assert_eq!(notification_service_for(&DisplayServer::Unknown), None);
    }
}
