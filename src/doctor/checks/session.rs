//! Session environment check - detects Wayland variables left behind in the
//! systemd user manager by an earlier session.
//!
//! The user manager outlives graphical sessions. When a Wayland session
//! exports `WAYLAND_DISPLAY`/`XDG_SESSION_TYPE=wayland` and a later X11 session
//! does not replace them, every user service and D-Bus activated application
//! (terminals, portals, clipboard capture) believes it runs under Wayland.

use std::collections::HashMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command as TokioCommand;

use super::{CheckStatus, DoctorCheck, PrivilegeLevel};
use crate::common::display_server::wayland_socket_reachable;

#[derive(Default)]
pub struct SessionEnvironmentCheck;

#[derive(Debug, PartialEq, Eq)]
enum Diagnosis {
    Healthy,
    /// The manager points at a Wayland display nobody listens on.
    StaleWayland {
        display: String,
    },
}

fn diagnose(
    manager_env: &HashMap<String, String>,
    reachable: impl Fn(&str) -> Option<bool>,
) -> Diagnosis {
    match manager_env.get("WAYLAND_DISPLAY") {
        Some(display) if reachable(display) == Some(false) => Diagnosis::StaleWayland {
            display: display.clone(),
        },
        _ => Diagnosis::Healthy,
    }
}

fn parse_environment(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

async fn manager_environment() -> Result<HashMap<String, String>> {
    let output = TokioCommand::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .await
        .context("Failed to run systemctl")?;
    anyhow::ensure!(output.status.success(), "systemctl show-environment failed");
    Ok(parse_environment(&String::from_utf8_lossy(&output.stdout)))
}

async fn run_command(program: &str, args: &[&str]) -> Result<()> {
    let status = TokioCommand::new(program)
        .args(args)
        .status()
        .await
        .with_context(|| format!("Failed to run {program}"))?;
    anyhow::ensure!(status.success(), "{program} failed with {status}");
    Ok(())
}

#[async_trait]
impl DoctorCheck for SessionEnvironmentCheck {
    fn name(&self) -> &'static str {
        "Session Environment"
    }

    fn id(&self) -> &'static str {
        "session-environment"
    }

    fn check_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    fn fix_privilege_level(&self) -> PrivilegeLevel {
        PrivilegeLevel::User
    }

    async fn execute(&self) -> CheckStatus {
        let manager_env = match manager_environment().await {
            Ok(env) => env,
            Err(_) => {
                return CheckStatus::Skipped("systemd user manager not available".to_string());
            }
        };

        match diagnose(&manager_env, wayland_socket_reachable) {
            Diagnosis::Healthy => {
                CheckStatus::Pass("systemd user environment matches the session".to_string())
            }
            Diagnosis::StaleWayland { display } => CheckStatus::Fail {
                message: format!(
                    "systemd user environment has WAYLAND_DISPLAY={display}, but no compositor \
                     is listening there; services and D-Bus activated apps will wrongly assume Wayland"
                ),
                fixable: true,
            },
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some(
            "Remove the stale Wayland variables from the systemd user environment. \
             Already running apps keep their environment until restarted."
                .to_string(),
        )
    }

    async fn fix(&self) -> Result<()> {
        let manager_env = manager_environment().await?;
        run_command(
            "systemctl",
            &[
                "--user",
                "unset-environment",
                "WAYLAND_DISPLAY",
                "WAYLAND_SOCKET",
            ],
        )
        .await?;

        // Without a Wayland display, an X11 session is the only one left.
        let has_display = manager_env.contains_key("DISPLAY");
        if has_display && manager_env.get("XDG_SESSION_TYPE").map(String::as_str) == Some("wayland")
        {
            run_command(
                "dbus-update-activation-environment",
                &["--systemd", "XDG_SESSION_TYPE=x11"],
            )
            .await?;
        } else if !has_display {
            run_command(
                "systemctl",
                &["--user", "unset-environment", "XDG_SESSION_TYPE"],
            )
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn parses_show_environment_output() {
        let parsed = parse_environment("DISPLAY=:0\nWAYLAND_DISPLAY=wayland-1\nweird line\n");
        assert_eq!(parsed.get("DISPLAY").map(String::as_str), Some(":0"));
        assert_eq!(
            parsed.get("WAYLAND_DISPLAY").map(String::as_str),
            Some("wayland-1")
        );
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn dead_wayland_display_is_stale() {
        let manager = env(&[("WAYLAND_DISPLAY", "wayland-1"), ("DISPLAY", ":0")]);
        assert_eq!(
            diagnose(&manager, |_| Some(false)),
            Diagnosis::StaleWayland {
                display: "wayland-1".into()
            }
        );
    }

    #[test]
    fn live_or_absent_wayland_display_is_healthy() {
        let manager = env(&[("WAYLAND_DISPLAY", "wayland-1")]);
        assert_eq!(diagnose(&manager, |_| Some(true)), Diagnosis::Healthy);
        // Unresolvable (e.g. no XDG_RUNTIME_DIR) is not proof of staleness.
        assert_eq!(diagnose(&manager, |_| None), Diagnosis::Healthy);
        assert_eq!(
            diagnose(&env(&[("DISPLAY", ":0")]), |_| Some(false)),
            Diagnosis::Healthy
        );
    }
}
