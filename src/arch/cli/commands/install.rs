use std::io::IsTerminal;

use anyhow::Result;
use colored::Colorize;

use crate::arch::cli::ArchCommands;
use crate::arch::host::HostProfile;

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
            .default_no()
            .confirm_dialog()?,
        ConfirmResult::Yes
    ))
}

/// A network configuration tool the live environment may ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkSetupTool {
    Nmtui,
    Connmanctl,
    Iwctl,
}

impl NetworkSetupTool {
    fn binary(self) -> &'static str {
        match self {
            Self::Nmtui => "nmtui",
            Self::Connmanctl => "connmanctl",
            Self::Iwctl => "iwctl",
        }
    }

    fn action_label(self) -> &'static str {
        match self {
            Self::Nmtui => "Open nmtui",
            Self::Connmanctl => "Open connmanctl",
            Self::Iwctl => "Open iwctl",
        }
    }
}

/// The first available network tool, in preference order.
///
/// `available` is injected so the order is testable without touching the
/// machine's installed binaries.
fn select_network_setup_tool(available: impl Fn(&str) -> bool) -> Option<NetworkSetupTool> {
    [
        NetworkSetupTool::Nmtui,
        NetworkSetupTool::Connmanctl,
        NetworkSetupTool::Iwctl,
    ]
    .into_iter()
    .find(|tool| available(tool.binary()))
}

fn run_network_setup_tool(tool: NetworkSetupTool) -> Result<()> {
    let status = std::process::Command::new(tool.binary()).status()?;
    if !status.success() {
        eprintln!(
            "{} exited unsuccessfully; internet connectivity will be checked again.",
            tool.binary()
        );
    }
    Ok(())
}

fn wait_for_manual_connection() -> Result<()> {
    println!("No supported network configuration tool was found.");
    println!("Connect to a network manually, then press Enter to check again.");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(())
}

async fn ensure_interactive_internet() -> Result<bool> {
    use crate::menu_utils::{ConfirmResult, FzfWrapper};

    // Offline installs never block on connectivity: the bundle covers the
    // packages, so the network prompt stays an offer the user may take or
    // leave.
    let offline = crate::arch::offline::mode().is_offline();

    // Check HTTPS: ICMP may be blocked on a working connection.
    while !crate::arch::network::available().await? {
        let tool = select_network_setup_tool(|binary| which::which(binary).is_ok());
        let (prompt, no_text) = if offline && tool.is_some() {
            (
                "No internet connection was detected. The offline bundle covers this installation.\n\nOpen the network configuration anyway?",
                "Continue without network",
            )
        } else if offline {
            (
                "Cannot reach Arch Linux over HTTPS. The offline bundle covers this installation. Configure networking using this system's settings if needed, then retry, or continue without network.",
                "Continue without network",
            )
        } else if tool.is_some() {
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
            .yes_text(tool.map_or("Check again", NetworkSetupTool::action_label))
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

        match tool {
            Some(tool) => run_network_setup_tool(tool)?,
            None => wait_for_manual_connection()?,
        }
    }

    Ok(true)
}

/// Ask whether to discard a saved attempt that execution refused to resume.
///
/// Only a [`ResumeBlocked`] refusal is offered: any other failure keeps its own
/// diagnosis (and its log menu). This is the supported way out of the
/// fail-closed resume policy, which otherwise leaves the user to delete a
/// root-owned state file by hand.
fn offer_to_start_over(error: &anyhow::Error) -> Result<bool> {
    use crate::arch::execution::state::ResumeBlocked;
    use crate::menu_utils::{ConfirmResult, FzfWrapper};

    let Some(blocked) = error.downcast_ref::<ResumeBlocked>() else {
        return Ok(false);
    };

    Ok(matches!(
        FzfWrapper::builder()
            .confirm(format!(
                "The previous installation attempt cannot be resumed:\n\n{blocked}\n\n\
                 Starting over discards that attempt and prepares the target disk again.",
            ))
            .title("Previous installation attempt")
            .yes_text("Start over")
            .no_text("Exit")
            .default_no()
            .confirm_dialog()?,
        ConfirmResult::Yes
    ))
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

    let profile = HostProfile::detect()?;
    // The source already performed these checks before launching a bootstrap.
    // Network configuration belongs to the source's desktop/service manager;
    // neither is exposed in the disposable Arch environment.
    if profile != HostProfile::Bootstrap {
        if !confirm_battery_power()? {
            println!("Installation aborted.");
            return Ok(());
        }

        // Network configuration must run as the desktop user. Keep this before
        // privilege escalation and re-check after every network configuration session.
        if !ensure_interactive_internet().await? {
            return Ok(());
        }
    }

    if profile == HostProfile::ForeignDistro {
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
                "Arch Linux or Artix Linux installation is only supported on x86_64 architecture. Detected architecture: {}",
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

    let mut exec_result = Box::pin(super::exec::handle_exec_command(
        build_steps(),
        None,
        default_questions_file(),
        false,
    ))
    .await;

    // Execution deliberately refuses to reuse a checkpoint from another
    // configuration or from an interrupted disk preparation. Offer the reset
    // here rather than after the wizard, where re-asking every question would
    // be the only alternative.
    if let Err(error) = &exec_result
        && offer_to_start_over(error)?
    {
        crate::arch::execution::state::InstallState::discard()?;
        println!("Discarded the previous installation attempt. Starting over.");
        exec_result = Box::pin(super::exec::handle_exec_command(
            build_steps(),
            None,
            default_questions_file(),
            false,
        ))
        .await;
    }

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
    fn network_tools_are_preferred_in_order() {
        assert_eq!(
            select_network_setup_tool(|_| true),
            Some(NetworkSetupTool::Nmtui),
            "nmtui wins when everything is installed"
        );
        assert_eq!(
            select_network_setup_tool(|binary| binary != "nmtui"),
            Some(NetworkSetupTool::Connmanctl)
        );
        assert_eq!(
            select_network_setup_tool(|binary| binary == "iwctl"),
            Some(NetworkSetupTool::Iwctl)
        );
        assert_eq!(
            select_network_setup_tool(|_| false),
            None,
            "no tool falls back to the manual prompt"
        );
    }

    #[test]
    fn the_wizard_uses_a_host_relative_configuration_path() {
        // The path is resolved per host so a running system's own
        // `/etc/instant/questions.toml` is never the wizard's output.
        let path = default_questions_file();
        assert_eq!(path, crate::arch::execution::paths::host_questions_file());
        assert!(path.ends_with("questions.toml"));
    }
}
