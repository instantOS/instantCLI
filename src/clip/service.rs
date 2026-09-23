use std::process::Command;

use anyhow::{Context, Result};

use crate::assist::deps::XCLIP;
use crate::common::package::InstallResult;
use crate::common::systemd::{SystemdManager, ensure_graphical_session_target};
use crate::settings::deps::CLIPHIST;

use super::history::ClipBackend;

const WAYLAND_SERVICE: &str = "cliphist.service";
const X11_SERVICE_NAME: &str = "ins-clip-x11";
const X11_SERVICE: &str = "ins-clip-x11.service";
/// Former X11 backend. Text-only and ordered before DISPLAY is available.
const LEGACY_CLIPMENU_SERVICE: &str = "clipmenud.service";

/// Other clipboard managers that fight over selection ownership or record
/// duplicate history. Matched against `/proc/<pid>/comm`.
const CONFLICTING_MANAGERS: &[&str] = &[
    "clipmenud",
    "clipcatd",
    "greenclip",
    "copyq",
    "parcellite",
    "clipit",
    "diodon",
    "xfce4-clipman",
    "gpaste-daemon",
];

#[derive(Debug, Clone)]
pub struct ClipServiceStatus {
    pub backend: ClipBackend,
    pub installed: bool,
    pub enabled: bool,
    pub active: bool,
    pub conflicts: Vec<String>,
}

fn service_name(backend: ClipBackend) -> &'static str {
    match backend {
        ClipBackend::Wayland => WAYLAND_SERVICE,
        ClipBackend::X11 => X11_SERVICE,
    }
}

fn dependencies_installed(backend: ClipBackend) -> bool {
    CLIPHIST.is_installed() && (backend != ClipBackend::X11 || XCLIP.is_installed())
}

pub fn status(backend: ClipBackend) -> ClipServiceStatus {
    let installed = dependencies_installed(backend);
    let systemd = SystemdManager::user();
    let service = service_name(backend);
    ClipServiceStatus {
        backend,
        installed,
        enabled: installed && systemd.is_enabled(service),
        active: installed && systemd.is_active(service),
        conflicts: running_conflicts(),
    }
}

/// Install dependencies and start capture for `backend`. Returns `false`
/// when the user cancelled the installation.
///
/// Both capture services write to the same cliphist database and are bound
/// to `graphical-session.target`, so the other session type's service is left
/// alone: users switching between X11 and Wayland keep capture in both.
pub fn enable(backend: ClipBackend) -> Result<bool> {
    if !ensure_installed(&CLIPHIST)? {
        return Ok(false);
    }
    if backend == ClipBackend::X11 && !ensure_installed(&XCLIP)? {
        return Ok(false);
    }

    let systemd = SystemdManager::user();
    disable_if_present(&systemd, LEGACY_CLIPMENU_SERVICE)?;
    write_wayland_condition(&systemd)?;

    let unit_changed = if backend == ClipBackend::X11 {
        write_x11_unit(&systemd)?
    } else {
        false
    };

    // Both units require graphical-session.target, which lightweight window
    // managers do not always activate themselves.
    if !systemd.is_active("graphical-session.target") {
        ensure_graphical_session_target()?;
    }

    let service = service_name(backend);
    if !systemd.is_enabled(service) {
        systemd.enable(service)?;
    }
    if !systemd.is_active(service) {
        systemd.start(service)?;
    } else if unit_changed {
        systemd.restart(service)?;
    }
    // A unit whose start condition fails is skipped without an error.
    anyhow::ensure!(
        systemd.is_active(service),
        "{service} did not start: the systemd user environment does not match this {} \
         session. Run `ins doctor run session-environment` to inspect it.",
        match backend {
            ClipBackend::Wayland => "Wayland",
            ClipBackend::X11 => "X11",
        }
    );
    Ok(true)
}

/// Stop capture in every session type.
pub fn disable() -> Result<()> {
    let systemd = SystemdManager::user();
    for service in [WAYLAND_SERVICE, X11_SERVICE, LEGACY_CLIPMENU_SERVICE] {
        disable_if_present(&systemd, service)?;
    }
    Ok(())
}

fn ensure_installed(dependency: &'static crate::common::package::Dependency) -> Result<bool> {
    Ok(matches!(
        dependency.ensure()?,
        InstallResult::Installed | InstallResult::AlreadyInstalled
    ))
}

fn disable_if_present(systemd: &SystemdManager, service: &str) -> Result<()> {
    if systemd.is_enabled(service) || systemd.is_active(service) {
        systemd.disable_and_stop(service)?;
    }
    Ok(())
}

/// Make the packaged `cliphist.service` skip X11 sessions instead of failing
/// there: it is bound to `graphical-session.target`, which X11 sessions start
/// too, but `wl-paste` can only work while a Wayland display is exported.
fn write_wayland_condition(systemd: &SystemdManager) -> Result<()> {
    let path = user_unit_dir()?
        .join(format!("{WAYLAND_SERVICE}.d"))
        .join("ins-wayland-only.conf");
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == WAYLAND_CONDITION) {
        return Ok(());
    }
    let parent = path.parent().context("Drop-in path has no parent")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create {}", parent.display()))?;
    std::fs::write(&path, WAYLAND_CONDITION)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    systemd.daemon_reload()?;
    // Clear a start-limit failure left behind by earlier X11 logins.
    let _ = Command::new("systemctl")
        .args(["--user", "reset-failed", WAYLAND_SERVICE])
        .status();
    Ok(())
}

const WAYLAND_CONDITION: &str = "[Unit]\nConditionEnvironment=WAYLAND_DISPLAY\n";

fn user_unit_dir() -> Result<std::path::PathBuf> {
    Ok(dirs::config_dir()
        .context("unable to determine user config directory")?
        .join("systemd/user"))
}

/// Write the X11 capture unit. Returns whether the file content changed.
fn write_x11_unit(systemd: &SystemdManager) -> Result<bool> {
    let executable = std::env::current_exe().context("Failed to locate the ins executable")?;
    let content = x11_unit_content(&executable.to_string_lossy());
    let path = user_unit_dir()?.join(X11_SERVICE);
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
        return Ok(false);
    }
    systemd.create_user_service_file(X11_SERVICE_NAME, &content)?;
    Ok(true)
}

fn x11_unit_content(executable: &str) -> String {
    format!(
        "[Unit]\n\
         Description=X11 clipboard history capture for cliphist (instantCLI)\n\
         PartOf=graphical-session.target\n\
         After=graphical-session.target\n\
         Requisite=graphical-session.target\n\
         ConditionEnvironment=DISPLAY\n\
         ConditionEnvironment=!WAYLAND_DISPLAY\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart=\"{executable}\" clip watch-x11\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=graphical-session.target\n"
    )
}

fn running_conflicts() -> Vec<String> {
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found: Vec<String> = processes
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("comm")).ok())
        .map(|comm| comm.trim().to_string())
        .filter(|comm| CONFLICTING_MANAGERS.contains(&comm.as_str()))
        .collect();
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_unit_is_bound_to_the_graphical_session() {
        let unit = x11_unit_content("/usr/bin/ins");
        assert!(unit.contains("ExecStart=\"/usr/bin/ins\" clip watch-x11"));
        assert!(unit.contains("PartOf=graphical-session.target"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
        assert!(!unit.contains("default.target"));
        assert!(unit.contains("ConditionEnvironment=!WAYLAND_DISPLAY"));
    }
}
