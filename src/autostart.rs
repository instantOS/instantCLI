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

        let mut file =
            match nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
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
    // Authentication must not depend on wallpaper, compositor setup, or network work.
    ensure_polkit_agent(debug).await;

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

    let display = DisplayServer::detect();
    let Some(agent) = polkit_agent_for(&display) else {
        return;
    };

    // Other desktops own their agents. In instantWM, start our selected service
    // idempotently instead of guessing registration from D-Bus process names.
    if CompositorType::detect() != CompositorType::InstantWM
        && crate::doctor::checks::security::is_polkit_agent_running().await
    {
        return;
    }

    if display == DisplayServer::Wayland {
        let manager = systemd::SystemdManager::user();
        if manager.service_exists("hyprpolkitagent.service") {
            if let Err(e) = start_polkit_service(std::path::Path::new("systemctl")) {
                eprintln!(
                    "Failed to start polkit agent: {e:#}. Check journalctl --user -b -u hyprpolkitagent.service"
                );
            } else if debug {
                println!("hyprpolkitagent.service is active (authentication not tested)");
            }
            // Do not bypass a failed/masked service with an unmanaged copy.
            return;
        }
    }

    // lxqt-policykit is installed by instantdepend; lxpolkit is a different package.
    if crate::doctor::checks::security::is_agent_process_running(agent).await {
        return;
    }
    match std::process::Command::new(agent).spawn() {
        Ok(mut child) => {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            match child.try_wait() {
                Ok(Some(status)) => {
                    eprintln!("Polkit agent {agent} exited during startup: {status}")
                }
                Err(e) => eprintln!("Could not check polkit agent {agent}: {e}"),
                Ok(None) => {
                    // Reap when it exits rather than leaking a zombie.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    if debug {
                        println!("Started {agent} (authentication not tested)");
                    }
                }
            }
        }
        Err(e) => eprintln!("Failed to start polkit agent {agent}: {e}"),
    }
}

fn polkit_agent_for(
    display: &crate::common::display_server::DisplayServer,
) -> Option<&'static str> {
    use crate::common::display_server::DisplayServer;
    match display {
        DisplayServer::Wayland => Some("hyprpolkitagent"),
        DisplayServer::X11 => Some("lxqt-policykit-agent"),
        DisplayServer::Unknown => None,
    }
}

fn start_polkit_service(systemctl: &std::path::Path) -> Result<()> {
    // ConditionEnvironment=WAYLAND_DISPLAY is evaluated by the user manager,
    // not against ins's environment. A skipped condition still makes start
    // return success, so import the display and explicitly check active state.
    let vars: Vec<_> = [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XAUTHORITY",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
    ]
    .into_iter()
    .filter(|name| std::env::var_os(name).is_some())
    .collect();
    if !vars.is_empty() {
        let status = std::process::Command::new(systemctl)
            .args(["--user", "import-environment"])
            .args(vars)
            .status()
            .context("Importing display environment into systemd")?;
        anyhow::ensure!(
            status.success(),
            "Could not import display environment into systemd"
        );
    }
    let status = std::process::Command::new(systemctl)
        .args(["--user", "start", "hyprpolkitagent.service"])
        .status()
        .context("Starting hyprpolkitagent.service")?;
    anyhow::ensure!(status.success(), "hyprpolkitagent.service failed to start");
    let status = std::process::Command::new(systemctl)
        .args(["--user", "is-active", "--quiet", "hyprpolkitagent.service"])
        .status()
        .context("Checking hyprpolkitagent.service")?;
    anyhow::ensure!(
        status.success(),
        "hyprpolkitagent.service did not become active (it may have been skipped or exited)"
    );
    Ok(())
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
    use super::AutostartGuard;
    use super::notification_service_for;
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
    fn polkit_uses_wayland_agent_only_on_wayland() {
        assert_eq!(
            super::polkit_agent_for(&DisplayServer::Wayland),
            Some("hyprpolkitagent")
        );
        assert_eq!(
            super::polkit_agent_for(&DisplayServer::X11),
            Some("lxqt-policykit-agent")
        );
        assert_eq!(super::polkit_agent_for(&DisplayServer::Unknown), None);
    }

    #[test]
    fn polkit_service_checks_active_state_even_when_start_succeeds() {
        use std::os::unix::fs::PermissionsExt;
        // Model systemd's condition-skip: start succeeds, is-active fails.
        // Also verify a real start failure is surfaced without checking active.
        for (start_exit, active_exit, succeeds) in [(0, 0, true), (0, 3, false), (1, 0, false)] {
            let dir = tempfile::tempdir().unwrap();
            let executable = dir.path().join("systemctl");
            let log = dir.path().join("calls");
            std::fs::write(&executable, format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$2\" in\n  import-environment) exit 0 ;;\n  start) exit {start_exit} ;;\n  is-active) exit {active_exit} ;;\n  *) exit 99 ;;\nesac\n",
                log.display()
            )).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(super::start_polkit_service(&executable).is_ok(), succeeds);
            let calls = std::fs::read_to_string(log).unwrap();
            let lines: Vec<_> = calls.lines().collect();
            let start = lines
                .iter()
                .position(|line| *line == "--user start hyprpolkitagent.service")
                .unwrap();
            if start > 0 {
                assert!(lines[0].starts_with("--user import-environment "));
            }
            assert_eq!(
                calls.contains("--user is-active --quiet hyprpolkitagent.service"),
                start_exit == 0
            );
        }
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
