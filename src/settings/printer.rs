use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::common::requirements::{InstallTest, RequiredPackage};
use crate::common::systemd::SystemdManager;
use crate::menu_utils::{ConfirmResult, FzfWrapper};

use super::context::SettingsContext;
use super::registry::SettingRequirement;

const CUPS_SERVICE: &str = "cups";
const AVAHI_SERVICE: &str = "avahi-daemon";

pub const CUPS_PACKAGE: RequiredPackage = RequiredPackage {
    name: "CUPS print server",
    arch_package_name: Some("cups"),
    ubuntu_package_name: Some("cups"),
    tests: &[InstallTest::WhichSucceeds("cupsd")],
};

pub const CUPSBROWSING_PACKAGE: RequiredPackage = RequiredPackage {
    name: "cups-browsed printer discovery",
    arch_package_name: Some("cups-browsed"),
    ubuntu_package_name: Some("cups-browsed"),
    tests: &[InstallTest::WhichSucceeds("cups-browsed")],
};

pub const SYSTEM_CONFIG_PRINTER_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Printer configuration utility",
    arch_package_name: Some("system-config-printer"),
    ubuntu_package_name: Some("system-config-printer"),
    tests: &[InstallTest::WhichSucceeds("system-config-printer")],
};
const NSSWITCH_PATH: &str = "/etc/nsswitch.conf";

const RECOMMENDED_HOSTS_LINE: &str =
    "hosts: mymachines mdns_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] files myhostname dns";

const LEGACY_HOSTS_PATTERNS: &[&str] = &[" files ", " mdns "];

pub fn ensure_printer_packages(ctx: &mut SettingsContext) -> Result<bool> {
    let required = [CUPS_PACKAGE, CUPSBROWSING_PACKAGE, SYSTEM_CONFIG_PRINTER_PACKAGE];
    ctx.ensure_packages(&required)
}

pub fn launch_printer_manager(ctx: &mut SettingsContext) -> Result<()> {
    if !ensure_printer_packages(ctx)? {
        ctx.emit_info(
            "settings.printer.installation_cancelled",
            "Printer support setup was cancelled.",
        );
        return Ok(());
    }

    let status = std::process::Command::new("system-config-printer").status();
    if let Err(err) = status {
        anyhow::bail!("Failed to start system-config-printer: {err}");
    }
    Ok(())
}

pub fn configure_printer_support(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let systemd = SystemdManager::system_with_sudo();

    if enabled {
        if !ensure_printer_packages(ctx)? {
            ctx.emit_info(
                "settings.printer.enable.cancelled",
                "Printer service enablement cancelled.",
            );
            return Ok(());
        }

        update_nsswitch_if_needed(ctx)?;

        if !systemd.is_enabled(CUPS_SERVICE) || !systemd.is_active(CUPS_SERVICE) {
            systemd.enable_and_start(CUPS_SERVICE)?;
        }

        if !systemd.is_enabled(AVAHI_SERVICE) || !systemd.is_active(AVAHI_SERVICE) {
            systemd.enable_and_start(AVAHI_SERVICE)?;
        }

        ctx.notify(
            "Printer support",
            "CUPS and Avahi services enabled for network printing.",
        );
    } else {
        let result = FzfWrapper::builder()
            .confirm("Disable printer services? Jobs will stop printing.")
            .yes_text("Disable")
            .no_text("Cancel")
            .show_confirmation()?;

        if result != ConfirmResult::Yes {
            return Ok(());
        }

        if systemd.is_enabled(CUPS_SERVICE) || systemd.is_active(CUPS_SERVICE) {
            systemd.disable_and_stop(CUPS_SERVICE)?;
        }

        if systemd.is_enabled(AVAHI_SERVICE) || systemd.is_active(AVAHI_SERVICE) {
            systemd.disable_and_stop(AVAHI_SERVICE)?;
        }

        ctx.notify("Printer support", "CUPS and Avahi services disabled.");
    }

    Ok(())
}

fn update_nsswitch_if_needed(ctx: &mut SettingsContext) -> Result<()> {
    let path = Path::new(NSSWITCH_PATH);
    let contents = fs::read_to_string(path).with_context(|| format!(
        "Failed to read {}",
        path.display()
    ))?;

    if contents.lines().any(|line| line.trim_start().starts_with("hosts:")) {
        if contents
            .lines()
            .any(|line| line.trim() == RECOMMENDED_HOSTS_LINE)
        {
            return Ok(());
        }

        if contents
            .lines()
            .any(|line| LEGACY_HOSTS_PATTERNS.iter().all(|pat| line.contains(pat)))
        {
            let message = format!(
                "The current hosts line in {} uses deprecated mDNS configuration.\n\n{}
Recommends replacing existing entry with:\n{}",
                NSSWITCH_PATH, char::from(crate::ui::prelude::NerdFont::Info), RECOMMENDED_HOSTS_LINE
            );

            let result = FzfWrapper::builder()
                .confirm(&message)
                .yes_text("Update")
                .no_text("Skip")
                .show_confirmation()?;

            if result != ConfirmResult::Yes {
                ctx.emit_info(
                    "settings.printer.nsswitch.skipped",
                    "Skipped updating mDNS hosts configuration.",
                );
                return Ok(());
            }

            apply_nsswitch_update(&contents)?;
            ctx.notify("Printer discovery", "Updated mDNS configuration for printer discovery.");
        }
    }

    Ok(())
}

fn apply_nsswitch_update(current: &str) -> Result<()> {
    let mut temp = NamedTempFile::new().context("creating temporary nsswitch copy")?;
    for line in current.lines() {
        if line.trim_start().starts_with("hosts:") {
            writeln!(temp, "{}", RECOMMENDED_HOSTS_LINE)
                .context("writing updated hosts line")?;
        } else {
            writeln!(temp, "{}", line).context("writing nsswitch line")?;
        }
    }

    temp.flush().context("flushing updated nsswitch")?;
    temp.as_file()
        .sync_all()
        .context("syncing updated nsswitch")?;

    let status = std::process::Command::new("sudo")
        .arg("cp")
        .arg(temp.path())
        .arg(NSSWITCH_PATH)
        .status()
        .context("applying nsswitch update with sudo cp")?;

    if !status.success() {
        anyhow::bail!("failed to update {}", NSSWITCH_PATH);
    }

    Ok(())
}
