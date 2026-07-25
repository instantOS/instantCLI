use anyhow::Result;

use crate::common::package::{Dependency, InstallResult};
use crate::common::systemd::SystemdManager;
use crate::settings::deps::{CLIPHIST, CLIPMENU};

use super::history::ClipBackend;

#[derive(Debug, Clone)]
pub struct ClipServiceStatus {
    pub backend: ClipBackend,
    pub installed: bool,
    pub enabled: bool,
    pub active: bool,
}

fn service_name(backend: ClipBackend) -> &'static str {
    match backend {
        ClipBackend::Cliphist => "cliphist.service",
        ClipBackend::Clipmenu => "clipmenud.service",
    }
}

fn dependency(backend: ClipBackend) -> &'static Dependency {
    match backend {
        ClipBackend::Cliphist => &CLIPHIST,
        ClipBackend::Clipmenu => &CLIPMENU,
    }
}

pub fn status(backend: ClipBackend) -> ClipServiceStatus {
    let installed = dependency(backend).is_installed();
    let systemd = SystemdManager::user();
    let service = service_name(backend);
    ClipServiceStatus {
        backend,
        installed,
        enabled: installed && systemd.is_enabled(service),
        active: installed && systemd.is_active(service),
    }
}

pub fn enable(backend: ClipBackend) -> Result<bool> {
    match dependency(backend).ensure()? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => {}
        _ => return Ok(false),
    }

    // Do not leave an incompatible backend fighting for clipboard ownership.
    let other = match backend {
        ClipBackend::Cliphist => ClipBackend::Clipmenu,
        ClipBackend::Clipmenu => ClipBackend::Cliphist,
    };
    disable_if_present(other)?;

    let systemd = SystemdManager::user();
    let service = service_name(backend);
    if !systemd.is_enabled(service) {
        systemd.enable_and_start(service)?;
    } else if !systemd.is_active(service) {
        systemd.start(service)?;
    }
    Ok(true)
}

pub fn disable(backend: ClipBackend) -> Result<()> {
    disable_if_present(backend)
}

fn disable_if_present(backend: ClipBackend) -> Result<()> {
    if !dependency(backend).is_installed() {
        return Ok(());
    }
    let systemd = SystemdManager::user();
    let service = service_name(backend);
    if systemd.is_enabled(service) || systemd.is_active(service) {
        systemd.disable_and_stop(service)?;
    }
    Ok(())
}
