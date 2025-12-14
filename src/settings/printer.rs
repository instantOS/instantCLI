use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::common::package::{InstallResult, ensure_all};
use crate::common::systemd::SystemdManager;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::prelude::NerdFont;

use super::context::SettingsContext;
use super::deps::PRINTER_DEPS;
use super::store::BoolSettingKey;

const CUPS_SERVICE: &str = "cups";
const AVAHI_SERVICE: &str = "avahi-daemon";

pub const PRINTER_SERVICES_KEY: BoolSettingKey = BoolSettingKey::new("printers.services", false);

const NSSWITCH_PATH: &str = "/etc/nsswitch.conf";

const RECOMMENDED_HOSTS_LINE: &str = "hosts: mymachines mdns_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] files myhostname dns";

const ALTERNATIVE_RECOMMENDED_LINE: &str = "hosts: mymachines resolve [!UNAVAIL=return] mdns_minimal [NOTFOUND=return] files myhostname dns";

const LEGACY_HOSTS_PATTERNS: &[&str] = &["hosts:", " mdns"];

pub fn ensure_printer_packages(ctx: &mut SettingsContext) -> Result<bool> {
    match ensure_all(PRINTER_DEPS)? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => Ok(true),
        _ => {
            ctx.emit_info(
                "settings.printer.installation_cancelled",
                "Printer support setup was cancelled.",
            );
            Ok(false)
        }
    }
}

pub fn launch_printer_manager(ctx: &mut SettingsContext) -> Result<()> {
    if !ensure_printer_packages(ctx)? {
        ctx.notify("Printer manager", "Required printer packages missing.");
        return Ok(());
    }

    // Launch in detached mode (non-blocking) with redirected output
    Command::new("system-config-printer")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to launch system-config-printer")?;

    ctx.emit_success(
        "settings.printer.manager.launched",
        "Opened system-config-printer.",
    );

    Ok(())
}

pub fn configure_printer_support(ctx: &mut SettingsContext, enabled: bool) -> Result<()> {
    let systemd = SystemdManager::system_with_sudo();

    if enabled {
        if !ensure_printer_packages(ctx)? {
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

/// Represents the result of analyzing nsswitch configuration
#[derive(Debug, PartialEq)]
pub enum NsswitchAnalysis {
    /// No hosts line found
    NoHostsLine,
    /// Already configured correctly
    AlreadyConfigured,
    /// Needs mDNS configuration update
    NeedsUpdate {
        current_line: String,
        recommended_line: String,
    },
    /// Has mDNS but may need improvement
    HasMdns {
        current_line: String,
        can_improve: bool,
    },
}

/// Analyze nsswitch configuration to determine if mDNS setup is needed
pub fn analyze_nsswitch_config(contents: &str) -> NsswitchAnalysis {
    let hosts_line = contents
        .lines()
        .find(|line| line.trim_start().starts_with("hosts:"));

    let Some(hosts_line) = hosts_line else {
        return NsswitchAnalysis::NoHostsLine;
    };

    let trimmed = hosts_line.trim();

    // Check if already configured with our recommended line
    if trimmed == RECOMMENDED_HOSTS_LINE || trimmed == ALTERNATIVE_RECOMMENDED_LINE {
        return NsswitchAnalysis::AlreadyConfigured;
    }

    // Check if mDNS is already configured
    if trimmed.contains("mdns") {
        // Has mDNS but may not be optimal
        let can_improve = is_legacy_hosts_line(trimmed);
        return NsswitchAnalysis::HasMdns {
            current_line: trimmed.to_string(),
            can_improve,
        };
    }

    // No mDNS configured - needs update
    let recommended_line = if trimmed.contains("resolve [!UNAVAIL=return]") {
        // User has systemd-resolved, put mdns after resolve
        ALTERNATIVE_RECOMMENDED_LINE
    } else {
        // Standard recommendation
        RECOMMENDED_HOSTS_LINE
    };

    NsswitchAnalysis::NeedsUpdate {
        current_line: trimmed.to_string(),
        recommended_line: recommended_line.to_string(),
    }
}

/// Check if a hosts line uses legacy mDNS configuration
fn is_legacy_hosts_line(line: &str) -> bool {
    LEGACY_HOSTS_PATTERNS
        .iter()
        .all(|pattern| line.contains(pattern))
        && !line.contains("resolve [!UNAVAIL=return]")
        && !line.contains("mdns_minimal [NOTFOUND=return]")
}

/// Generate updated nsswitch content with mDNS configuration
pub fn generate_nsswitch_update(current: &str, recommended_line: &str) -> Result<String> {
    let mut result = String::new();

    for line in current.lines() {
        if line.trim_start().starts_with("hosts:") {
            result.push_str(recommended_line);
            result.push('\n');
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    Ok(result)
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

    let analysis = analyze_nsswitch_config(&contents);

    match analysis {
        NsswitchAnalysis::NoHostsLine => {
            ctx.emit_info(
                "settings.printer.nsswitch.no_hosts_line",
                "No hosts line found in nsswitch.conf.",
            );
        }
        NsswitchAnalysis::AlreadyConfigured => {
            // Already configured correctly
            return Ok(());
        }
        NsswitchAnalysis::HasMdns {
            current_line,
            can_improve: false,
        } => {
            ctx.emit_info(
                "settings.printer.nsswitch.already_configured",
                &format!("mDNS already configured: {}", current_line),
            );
            return Ok(());
        }
        NsswitchAnalysis::HasMdns {
            current_line,
            can_improve: true,
        } => {
            let message = format!(
                "The current hosts line in {} may not be optimal for printer discovery.\n\n{} {}
Recommended improvement:\n{}",
                NSSWITCH_PATH,
                char::from(NerdFont::Info),
                current_line,
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

            apply_nsswitch_update(ctx, &contents, RECOMMENDED_HOSTS_LINE)?;
            ctx.notify(
                "Printer discovery",
                "Updated mDNS configuration for driverless printers.",
            );
        }
        NsswitchAnalysis::NeedsUpdate {
            current_line,
            recommended_line,
        } => {
            let message = format!(
                "The current hosts line in {} lacks mDNS support for printer discovery.\n\n{} {}
Recommended replacement:\n{}",
                NSSWITCH_PATH,
                char::from(NerdFont::Info),
                current_line,
                recommended_line
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

            apply_nsswitch_update(ctx, &contents, &recommended_line)?;
            ctx.notify(
                "Printer discovery",
                "Updated mDNS configuration for driverless printers.",
            );
        }
    }

    Ok(())
}

fn apply_nsswitch_update(
    ctx: &mut SettingsContext,
    current: &str,
    recommended_line: &str,
) -> Result<()> {
    let updated_content = generate_nsswitch_update(current, recommended_line)?;

    let mut temp = NamedTempFile::new().context("creating temporary nsswitch copy")?;
    temp.write_all(updated_content.as_bytes())
        .context("writing updated nsswitch")?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_nsswitch_config_no_hosts_line() {
        let config = r"# Test config
passwd: files
group: files";

        assert_eq!(
            analyze_nsswitch_config(config),
            NsswitchAnalysis::NoHostsLine
        );
    }

    #[test]
    fn test_analyze_nsswitch_config_already_configured() {
        let config = r"# Test config
passwd: files
hosts: mymachines mdns_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] files myhostname dns
group: files";

        assert_eq!(
            analyze_nsswitch_config(config),
            NsswitchAnalysis::AlreadyConfigured
        );
    }

    #[test]
    fn test_analyze_nsswitch_config_alternative_configured() {
        let config = r"# Test config
passwd: files
hosts: mymachines resolve [!UNAVAIL=return] mdns_minimal [NOTFOUND=return] files myhostname dns
group: files";

        assert_eq!(
            analyze_nsswitch_config(config),
            NsswitchAnalysis::AlreadyConfigured
        );
    }

    #[test]
    fn test_analyze_nsswitch_config_current_user_config() {
        let config = r"# Name Service Switch configuration file.
# See nsswitch.conf(5) for details.

passwd: files systemd
group: files [SUCCESS=merge] systemd
shadow: files systemd
gshadow: files systemd

publickey: files

hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns
networks: files

protocols: files
services: files
ethers: files
rpc: files

netgroup: files";

        let expected = NsswitchAnalysis::NeedsUpdate {
            current_line: "hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns"
                .to_string(),
            recommended_line: ALTERNATIVE_RECOMMENDED_LINE.to_string(),
        };

        assert_eq!(analyze_nsswitch_config(config), expected);
    }

    #[test]
    fn test_analyze_nsswitch_config_basic_no_mdns() {
        let config = r"# Test config
passwd: files
hosts: files dns
group: files";

        let expected = NsswitchAnalysis::NeedsUpdate {
            current_line: "hosts: files dns".to_string(),
            recommended_line: RECOMMENDED_HOSTS_LINE.to_string(),
        };

        assert_eq!(analyze_nsswitch_config(config), expected);
    }

    #[test]
    fn test_analyze_nsswitch_config_has_legacy_mdns() {
        let config = r"# Test config
passwd: files
hosts: files mdns dns
group: files";

        let expected = NsswitchAnalysis::HasMdns {
            current_line: "hosts: files mdns dns".to_string(),
            can_improve: true,
        };

        assert_eq!(analyze_nsswitch_config(config), expected);
    }

    #[test]
    fn test_analyze_nsswitch_config_has_good_mdns() {
        let config = r"# Test config
passwd: files
hosts: files mdns_minimal [NOTFOUND=return] dns
group: files";

        let expected = NsswitchAnalysis::HasMdns {
            current_line: "hosts: files mdns_minimal [NOTFOUND=return] dns".to_string(),
            can_improve: false,
        };

        assert_eq!(analyze_nsswitch_config(config), expected);
    }

    #[test]
    fn test_generate_nsswitch_update_basic() {
        let current = r"# Test config
passwd: files
hosts: files dns
group: files";

        let expected = r"# Test config
passwd: files
hosts: mymachines mdns_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] files myhostname dns
group: files
";

        let result = generate_nsswitch_update(current, RECOMMENDED_HOSTS_LINE).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_generate_nsswitch_update_current_user() {
        let current = r"# Name Service Switch configuration file.
# See nsswitch.conf(5) for details.

passwd: files systemd
group: files [SUCCESS=merge] systemd
shadow: files systemd
gshadow: files systemd

publickey: files

hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns
networks: files

protocols: files
services: files
ethers: files
rpc: files

netgroup: files";

        let expected = r"# Name Service Switch configuration file.
# See nsswitch.conf(5) for details.

passwd: files systemd
group: files [SUCCESS=merge] systemd
shadow: files systemd
gshadow: files systemd

publickey: files

hosts: mymachines resolve [!UNAVAIL=return] mdns_minimal [NOTFOUND=return] files myhostname dns
networks: files

protocols: files
services: files
ethers: files
rpc: files

netgroup: files
";

        let result = generate_nsswitch_update(current, ALTERNATIVE_RECOMMENDED_LINE).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_generate_nsswitch_update_preserves_comments() {
        let current = r"# Test config
# This is a comment
passwd: files
# Another comment before hosts
hosts: files dns  # inline comment
group: files";

        let expected = r"# Test config
# This is a comment
passwd: files
# Another comment before hosts
hosts: mymachines mdns_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] files myhostname dns
group: files
";

        let result = generate_nsswitch_update(current, RECOMMENDED_HOSTS_LINE).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_is_legacy_hosts_line() {
        assert!(is_legacy_hosts_line("hosts: files mdns dns"));
        assert!(is_legacy_hosts_line("hosts: mdns files dns"));
        assert!(!is_legacy_hosts_line(
            "hosts: files mdns_minimal [NOTFOUND=return] dns"
        ));
        assert!(!is_legacy_hosts_line(
            "hosts: mymachines mdns_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] files myhostname dns"
        ));
        assert!(!is_legacy_hosts_line("hosts: files dns"));
        assert!(!is_legacy_hosts_line("hosts: files"));
    }

    #[test]
    fn test_recommended_lines() {
        assert!(!RECOMMENDED_HOSTS_LINE.is_empty());
        assert!(!ALTERNATIVE_RECOMMENDED_LINE.is_empty());
        assert!(RECOMMENDED_HOSTS_LINE.contains("mdns_minimal"));
        assert!(ALTERNATIVE_RECOMMENDED_LINE.contains("mdns_minimal"));
        assert!(RECOMMENDED_HOSTS_LINE != ALTERNATIVE_RECOMMENDED_LINE);
    }

    #[test]
    fn test_demo_current_user_analysis() {
        // This test demonstrates what would happen with your current config
        let your_config = r"# Name Service Switch configuration file.
# See nsswitch.conf(5) for details.

passwd: files systemd
group: files [SUCCESS=merge] systemd
shadow: files systemd
gshadow: files systemd

publickey: files

hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns
networks: files

protocols: files
services: files
ethers: files
rpc: files

netgroup: files";

        let expected = NsswitchAnalysis::NeedsUpdate {
            current_line: "hosts: mymachines resolve [!UNAVAIL=return] files myhostname dns"
                .to_string(),
            recommended_line: ALTERNATIVE_RECOMMENDED_LINE.to_string(),
        };

        assert_eq!(analyze_nsswitch_config(your_config), expected);

        // Test what the updated config would look like
        let result = generate_nsswitch_update(your_config, ALTERNATIVE_RECOMMENDED_LINE).unwrap();
        assert!(result.contains("hosts: mymachines resolve [!UNAVAIL=return] mdns_minimal [NOTFOUND=return] files myhostname dns"));

        // Verify only the hosts line changed
        let lines_before: Vec<&str> = your_config.lines().collect();
        let lines_after: Vec<&str> = result.lines().collect();

        for (i, line_before) in lines_before.iter().enumerate() {
            if i < lines_after.len() {
                let line_after = lines_after[i];
                if line_before.trim_start().starts_with("hosts:") {
                    assert_ne!(*line_before, line_after); // Should have changed
                    assert!(line_after.contains("mdns_minimal")); // Should contain mDNS
                } else {
                    assert_eq!(*line_before, line_after); // Should be unchanged
                }
            }
        }
    }
}
