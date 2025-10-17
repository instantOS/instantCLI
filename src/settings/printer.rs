use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::common::requirements::{InstallTest, RequiredPackage};
use crate::common::systemd::SystemdManager;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::prelude::{Level, NerdFont, emit};

use super::context::SettingsContext;
use super::store::BoolSettingKey;

const CUPS_SERVICE: &str = "cups";
const AVAHI_SERVICE: &str = "avahi-daemon";

pub const PRINTER_SERVICES_KEY: BoolSettingKey = BoolSettingKey::new("printers.services", false);

pub const CUPS_PACKAGE: RequiredPackage = RequiredPackage {
    name: "CUPS print server",
    arch_package_name: Some("cups"),
    ubuntu_package_name: Some("cups"),
    tests: &[
        InstallTest::FileExists("/usr/bin/cupsd"),
        InstallTest::FileExists("/usr/sbin/cupsd"),
    ],
};

pub const CUPS_FILTERS_PACKAGE: RequiredPackage = RequiredPackage {
    name: "cups-filters driverless printing",
    arch_package_name: Some("cups-filters"),
    ubuntu_package_name: Some("cups-filters"),
    tests: &[InstallTest::WhichSucceeds("cupsfilter")],
};

pub const GHOSTSCRIPT_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Ghostscript renderer",
    arch_package_name: Some("ghostscript"),
    ubuntu_package_name: Some("ghostscript"),
    tests: &[InstallTest::WhichSucceeds("gs")],
};

pub const AVAHI_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Avahi discovery daemon",
    arch_package_name: Some("avahi"),
    ubuntu_package_name: Some("avahi-daemon"),
    tests: &[InstallTest::WhichSucceeds("avahi-daemon")],
};

pub const SYSTEM_CONFIG_PRINTER_PACKAGE: RequiredPackage = RequiredPackage {
    name: "Printer configuration utility",
    arch_package_name: Some("system-config-printer"),
    ubuntu_package_name: Some("system-config-printer"),
    tests: &[InstallTest::WhichSucceeds("system-config-printer")],
};

pub const NSS_MDNS_PACKAGE: RequiredPackage = RequiredPackage {
    name: "nss-mdns resolver",
    arch_package_name: Some("nss-mdns"),
    ubuntu_package_name: Some("libnss-mdns"),
    tests: &[
        InstallTest::FileExists("/usr/lib/libnss_mdns.so.2"),
        InstallTest::FileExists("/usr/lib/x86_64-linux-gnu/libnss_mdns.so.2"),
    ],
};

const NSSWITCH_PATH: &str = "/etc/nsswitch.conf";

const RECOMMENDED_HOSTS_LINE: &str = "hosts: mymachines mdns_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] files myhostname dns";

const LEGACY_HOSTS_PATTERNS: &[&str] = &["hosts:", " mdns"];

pub fn ensure_printer_packages(ctx: &mut SettingsContext) -> Result<bool> {
    let required = [
        CUPS_PACKAGE,
        CUPS_FILTERS_PACKAGE,
        GHOSTSCRIPT_PACKAGE,
        AVAHI_PACKAGE,
        SYSTEM_CONFIG_PRINTER_PACKAGE,
        NSS_MDNS_PACKAGE,
    ];
    ctx.ensure_packages(&required)
}

pub fn launch_printer_manager(ctx: &mut SettingsContext) -> Result<()> {
    if !ensure_printer_packages(ctx)? {
        ctx.emit_info(
            "settings.printer.installation_cancelled",
            "Printer support setup was cancelled.",
        );
        ctx.notify("Printer manager", "Required printer packages missing.");
        return Ok(());
    }

    match Command::new("system-config-printer").status() {
        Ok(status) if status.success() => ctx.emit_success(
            "settings.printer.manager.launched",
            "Opened system-config-printer.",
        ),
        Ok(status) => {
            emit(
                Level::Warn,
                "settings.printer.manager.exit_status",
                &format!(
                    "{} system-config-printer exited with status {:?}",
                    char::from(NerdFont::Warning),
                    status.code()
                ),
                None,
            );
        }
        Err(err) => anyhow::bail!("Failed to start system-config-printer: {err}"),
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
            ctx.set_bool(PRINTER_SERVICES_KEY, false);
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
        ctx.emit_success(
            "settings.printer.services.enabled",
            "Printer services are now active.",
        );
    } else {
        let result = FzfWrapper::builder()
            .confirm("Disable printer services? Jobs will stop printing.")
            .yes_text("Disable")
            .no_text("Cancel")
            .show_confirmation()?;

        if result != ConfirmResult::Yes {
            ctx.set_bool(PRINTER_SERVICES_KEY, true);
            return Ok(());
        }

        if systemd.is_enabled(CUPS_SERVICE) || systemd.is_active(CUPS_SERVICE) {
            systemd.disable_and_stop(CUPS_SERVICE)?;
        }

        if systemd.is_enabled(AVAHI_SERVICE) || systemd.is_active(AVAHI_SERVICE) {
            systemd.disable_and_stop(AVAHI_SERVICE)?;
        }

        ctx.notify("Printer support", "CUPS and Avahi services disabled.");
        ctx.emit_success(
            "settings.printer.services.disabled",
            "Printer services have been disabled.",
        );
    }

    Ok(())
}

fn update_nsswitch_if_needed(ctx: &mut SettingsContext) -> Result<()> {
    let path = Path::new(NSSWITCH_PATH);
    if !path.exists() {
        ctx.emit_info(
            "settings.printer.nsswitch.missing",
            "nsswitch.conf not found; skipped mDNS host configuration check.",
        );
        return Ok(());
    }

    let contents =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    if contents
        .lines()
        .any(|line| line.trim_start().starts_with("hosts:"))
    {
        if contents
            .lines()
            .any(|line| line.trim() == RECOMMENDED_HOSTS_LINE)
        {
            return Ok(());
        }

        if let Some(hosts_line) = contents
            .lines()
            .find(|line| line.trim_start().starts_with("hosts:"))
            && is_legacy_hosts_line(hosts_line)
        {
            let message = format!(
                "The current hosts line in {} may prevent driverless printer discovery.\n\n{} {}
Recommended replacement:\n{}",
                NSSWITCH_PATH,
                char::from(NerdFont::Info),
                hosts_line.trim(),
                RECOMMENDED_HOSTS_LINE
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

            apply_nsswitch_update(ctx, &contents)?;
            ctx.notify(
                "Printer discovery",
                "Updated mDNS configuration for driverless printers.",
            );
        }
    }

    Ok(())
}

fn is_legacy_hosts_line(line: &str) -> bool {
    LEGACY_HOSTS_PATTERNS
        .iter()
        .all(|pattern| line.contains(pattern))
        && !line.contains("resolve [!UNAVAIL=return]")
}

fn apply_nsswitch_update(ctx: &mut SettingsContext, current: &str) -> Result<()> {
    let mut temp = NamedTempFile::new().context("creating temporary nsswitch copy")?;
    for line in current.lines() {
        if line.trim_start().starts_with("hosts:") {
            writeln!(temp, "{}", RECOMMENDED_HOSTS_LINE).context("writing updated hosts line")?;
        } else {
            writeln!(temp, "{}", line).context("writing nsswitch line")?;
        }
    }

    temp.flush().context("flushing updated nsswitch")?;
    temp.as_file()
        .sync_all()
        .context("syncing updated nsswitch")?;

    let source_path = temp.path().to_path_buf();
    ctx.run_command_as_root(
        "install",
        [
            "-m",
            "644",
            source_path.to_string_lossy().as_ref(),
            NSSWITCH_PATH,
        ],
    )?;

    Ok(())
}
