use std::io::IsTerminal;

use anyhow::Result;
use colored::Colorize;

use crate::arch::cli::ArchCommands;

use super::super::utils::ensure_root;
use super::ask::{AskOutcome, handle_ask_command};
use super::{build_steps, default_questions_file, handle_arch_command};

fn confirm_battery_power() -> Result<bool> {
    use crate::menu_utils::{ConfirmResult, FzfWrapper};

    let discharging = std::process::Command::new("acpi")
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("discharging")
        })
        .unwrap_or(false);

    if !discharging {
        return Ok(true);
    }

    Ok(matches!(
        FzfWrapper::builder()
            .confirm(
                "The computer is running on battery power. Connect it to power or make sure it has enough charge before installing.\n\nContinue anyway?",
            )
            .yes_text("Continue")
            .no_text("Abort installation")
            .confirm_dialog()?,
        ConfirmResult::Yes
    ))
}

async fn ensure_interactive_internet() -> Result<bool> {
    use crate::menu_utils::{ConfirmResult, FzfWrapper};

    // Offline installs never block on connectivity: the bundle covers the
    // packages, so nmtui stays an offer the user may take or leave.
    let offline = crate::arch::offline::mode().is_offline();

    // Test the protocol the installer actually needs. ICMP can be blocked on
    // a working connection, and must not prevent an HTTPS installation.
    while !crate::arch::network::available().await? {
        let has_nmtui = which::which("nmtui").is_ok();
        let (prompt, no_text) = if offline && has_nmtui {
            (
                "No internet connection was detected. The offline bundle covers this installation.\n\nOpen the network configuration anyway?",
                "Continue without network",
            )
        } else if offline {
            (
                "Cannot reach Arch Linux over HTTPS. The offline bundle covers this installation. Configure networking using this system's settings if needed, then retry, or continue without network.",
                "Continue without network",
            )
        } else if has_nmtui {
            (
                "No internet connection was detected. instantOS installation requires internet.\n\nOpen the network configuration now?",
                "Abort installation",
            )
        } else {
            (
                "Cannot reach Arch Linux over HTTPS. Configure the connection using this system's network settings, then retry.",
                "Abort installation",
            )
        };

        let choice = FzfWrapper::builder()
            .confirm(prompt)
            .yes_text(if has_nmtui { "Open nmtui" } else { "Retry" })
            .no_text(no_text)
            .confirm_dialog()?;

        if choice != ConfirmResult::Yes {
            if offline {
                println!("Continuing without network; packages will come from the offline bundle.");
                return Ok(true);
            }
            println!("Installation aborted: no internet connection.");
            return Ok(false);
        }

        if has_nmtui {
            let status = std::process::Command::new("nmtui").status()?;
            if !status.success() {
                eprintln!(
                    "nmtui exited unsuccessfully; internet connectivity will be checked again."
                );
            }
        }
    }

    Ok(true)
}

/// Handle the Install command - orchestrates the full installation process
pub(super) async fn handle_install_command(debug: bool) -> Result<()> {
    // The installer is interactive. Graphical launchers should normally open a
    // terminal themselves, but keep this guard here so future entry points
    // cannot accidentally start an invisible TUI.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        let current_exe = std::env::current_exe()?;
        instantos_terminal::Launcher::new(current_exe.as_os_str())
            .arg("arch")
            .arg("install")
            .app_id("ins-install")
            .title("instantOS Installation")
            .spawn_detached()?;
        return Ok(());
    }

    let profile = crate::arch::host::HostProfile::detect()?;
    // The source already performed these checks before launching a bootstrap.
    // Network configuration belongs to the source's desktop/service manager;
    // neither is exposed in the disposable Arch environment.
    if profile != crate::arch::host::HostProfile::Bootstrap {
        if !confirm_battery_power()? {
            println!("Installation aborted.");
            return Ok(());
        }

        // Network configuration must run as the desktop user. Keep this before
        // privilege escalation and re-check after every nmtui session.
        if !ensure_interactive_internet().await? {
            return Ok(());
        }
    }

    if profile == crate::arch::host::HostProfile::ForeignDistro {
        ensure_root()?;
        return crate::arch::bootstrap::launch(crate::arch::bootstrap::Request::Install { debug })
            .await;
    }

    // Installation state and answers live below /etc, so escalate before
    // touching either. The nested ask/exec commands then see an already-root
    // process and do not need to relaunch halfway through the workflow.
    ensure_root()?;

    let system_info = crate::arch::engine::SystemInfo::detect();

    if system_info.architecture != "x86_64" {
        eprintln!(
            "{} {}",
            "Error:".red().bold(),
            format!(
                "Arch Linux installation is only supported on x86_64 architecture. Detected architecture: {}",
                system_info.architecture
            )
            .red()
        );
        return Ok(());
    }

    let questions = build_steps();
    match Box::pin(handle_ask_command(None, None, questions)).await? {
        AskOutcome::Completed => {}
        AskOutcome::Cancelled => return Ok(()),
    }

    let exec_result = Box::pin(super::exec::handle_exec_command(
        build_steps(),
        None,
        default_questions_file(),
        false,
    ))
    .await;

    if exec_result.is_err() {
        // Never upload implicitly after a failure. Keep the installer open so
        // the user can inspect the log or explicitly choose an upload scope.
        let context = crate::arch::engine::InstallContext::load(default_questions_file()).ok();
        if let Err(error) = crate::arch::logging::show_failed_install_log_menu(context.as_ref()) {
            eprintln!("Could not show log options: {error}");
        }
    }

    if matches!(
        exec_result?,
        crate::arch::execution::ExecutionOutcome::AlreadyInstalled
    ) {
        return Ok(());
    }

    Box::pin(handle_arch_command(ArchCommands::Finished, debug)).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wizard_uses_a_host_relative_configuration_path() {
        // The path is resolved per host so a running system's own
        // `/etc/instant/questions.toml` is never the wizard's output.
        let path = default_questions_file();
        assert_eq!(path, crate::arch::execution::paths::host_questions_file());
        assert!(path.ends_with("questions.toml"));
    }
}
