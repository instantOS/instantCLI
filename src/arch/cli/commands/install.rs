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

fn ensure_interactive_internet() -> Result<bool> {
    use crate::menu_utils::{ConfirmResult, FzfWrapper};

    // Offline installs never block on connectivity: the bundle covers the
    // packages, so the network prompt stays an offer the user may take or
    // leave.
    let offline = crate::arch::offline::mode().is_offline();

    while !crate::common::network::check_internet() {
        let tool = select_network_setup_tool(|binary| which::which(binary).is_ok());
        let (prompt, no_text) = if offline {
            (
                "No internet connection was detected. The offline bundle covers this installation.\n\nOpen the network configuration anyway?",
                "Continue without network",
            )
        } else {
            (
                "No internet connection was detected. instantOS installation requires internet.\n\nOpen the network configuration now?",
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

/// Explain supported hosts and the disk restriction on running systems.
fn unsupported_host_message(distro: &str) -> String {
    format!(
        "instantOS can be installed from the live ISO or from a running Arch Linux, Artix Linux \
         or instantOS system — but not yet from {distro}.\n\
         \n\
         Two ways forward:\n\
           •  Boot the instantOS or Artix live ISO on this machine and run the installer from there.\n\
           •  Install Arch Linux or Artix Linux first, boot it, then run `ins arch install` to put \
         instantOS on a different disk.\n\
         \n\
         From a running system, the disk you are running from cannot be used — the installer \
         refuses it and says so."
    )
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

    if !confirm_battery_power()? {
        println!("Installation aborted.");
        return Ok(());
    }

    // Network configuration must run as the desktop user. Keep this before
    // privilege escalation and re-check after every nmtui session.
    if !ensure_interactive_internet()? {
        return Ok(());
    }

    // Installation state and answers live below /etc, so escalate before
    // touching either. The nested ask/exec commands then see an already-root
    // process and do not need to relaunch halfway through the workflow.
    ensure_root()?;

    let system_info = crate::arch::engine::SystemInfo::detect();

    let profile = HostProfile::detect().unwrap_or_else(|error| {
        eprintln!("Warning: could not classify this system: {error:#}");
        HostProfile::ForeignDistro
    });
    if !profile.supports_installation() {
        eprintln!(
            "{} {}",
            "Error:".red().bold(),
            unsupported_host_message(&system_info.distro).red()
        );
        return Ok(());
    }

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
    fn the_distro_refusal_names_both_supported_hosts() {
        let rendered = unsupported_host_message("Ubuntu");

        assert!(rendered.contains("Ubuntu"));
        assert!(rendered.contains("live ISO"), "one supported host named");
        assert!(
            rendered.contains("Arch Linux"),
            "the Arch host is named: {rendered}"
        );
        assert!(
            rendered.contains("Artix Linux"),
            "the Artix host is named: {rendered}"
        );
        assert!(
            rendered.contains("Artix live ISO"),
            "the Artix live ISO is named: {rendered}"
        );
        assert!(
            rendered.contains("ins arch install"),
            "the command for the second way forward"
        );
        assert!(
            rendered.contains("different disk"),
            "and the scope limit of the running-system path: {rendered}"
        );
    }

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
