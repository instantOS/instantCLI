use anyhow::{Result, anyhow};

use super::types::PASS_OTP_DEP;
use crate::assist::deps::{GPG, LIBNOTIFY, PASS, WL_CLIPBOARD, XCLIP};
use crate::assist::utils::show_notification;
use crate::common::display_server::DisplayServer;
use crate::common::package::{Dependency, InstallResult, ensure_all};

pub(super) fn ensure_core_dependencies() -> Result<()> {
    ensure_dependencies(&[&PASS, &GPG], "pass")
}

pub(super) fn ensure_clipboard_dependencies() -> Result<()> {
    let mut deps: Vec<&'static Dependency> = vec![&PASS, &GPG];
    match DisplayServer::detect() {
        DisplayServer::Wayland => deps.push(&WL_CLIPBOARD),
        DisplayServer::X11 => deps.push(&XCLIP),
        DisplayServer::Unknown => {}
    }

    ensure_dependencies(&deps, "clipboard integration for pass")
}

pub(super) fn ensure_otp_dependency() -> Result<()> {
    ensure_dependencies(&[&PASS, &GPG, &PASS_OTP_DEP], "pass-otp")
}

fn ensure_dependencies(deps: &[&'static Dependency], label: &str) -> Result<()> {
    match ensure_all(deps)? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(()),
        InstallResult::Declined => Err(anyhow!("{label} installation cancelled")),
        InstallResult::NotAvailable { hint, .. } => {
            Err(anyhow!("{label} is not available on this system: {hint}"))
        }
        InstallResult::Failed { reason } => Err(anyhow!("{label} installation failed: {reason}")),
    }
}

pub(super) fn maybe_notify(title: &str, message: &str) {
    if LIBNOTIFY.is_installed() {
        let _ = show_notification(title, message);
    }
}
