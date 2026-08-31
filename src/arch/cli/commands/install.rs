use std::io::IsTerminal;

use anyhow::Result;
use colored::Colorize;

use crate::arch::cli::{ArchCommands, DEFAULT_QUESTIONS_FILE};

use super::super::utils::ensure_root;
use super::ask::{AskOutcome, handle_ask_command};
use super::{build_steps, handle_arch_command};

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

fn ensure_interactive_internet() -> Result<bool> {
    use crate::menu_utils::{ConfirmResult, FzfWrapper};

    while !crate::common::network::check_internet() {
        let choice = FzfWrapper::builder()
            .confirm(
                "No internet connection was detected. instantOS installation requires internet.\n\nOpen the network configuration now?",
            )
            .yes_text("Open nmtui")
            .no_text("Abort installation")
            .confirm_dialog()?;

        if choice != ConfirmResult::Yes {
            println!("Installation aborted: no internet connection.");
            return Ok(false);
        }

        let status = std::process::Command::new("nmtui").status()?;
        if !status.success() {
            eprintln!("nmtui exited unsuccessfully; internet connectivity will be checked again.");
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
        crate::common::terminal::TerminalLauncher::new(current_exe.to_string_lossy())
            .arg("arch")
            .arg("install")
            .class("ins-install")
            .title("instantOS Installation")
            .launch()?;
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

    // Check architecture
    let system_info = crate::arch::engine::SystemInfo::detect();

    // Check distro
    if !system_info.distro.contains("Arch") && !system_info.distro.contains("instantOS") {
        eprintln!(
            "{} {}",
            "Error:".red().bold(),
            format!(
                "Arch Linux installation is only supported on Arch Linux or instantOS. Detected distro: {}",
                system_info.distro
            )
            .red()
        );
        return Ok(());
    }

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

    // Mark start time
    let mut state = crate::arch::execution::state::InstallState::load()?;
    state.mark_start();
    state.save()?;

    // 1. Ask questions
    let questions = build_steps();
    match Box::pin(handle_ask_command(None, None, questions)).await? {
        AskOutcome::Completed => {}
        AskOutcome::Cancelled => return Ok(()),
    }

    // 2. Execute
    let exec_result = Box::pin(handle_arch_command(
        ArchCommands::Exec {
            step: None,
            questions_file: std::path::PathBuf::from(DEFAULT_QUESTIONS_FILE),
            dry_run: false,
        },
        debug,
    ))
    .await;

    if exec_result.is_err() {
        // Try to upload logs if forced or requested
        if let Ok(context) = crate::arch::engine::InstallContext::load(DEFAULT_QUESTIONS_FILE) {
            crate::arch::logging::process_log_upload(&context);
        } else if std::path::Path::new("/etc/instantos/uploadlogs").exists() {
            println!("Uploading installation logs (forced by /etc/instantos/uploadlogs)...");
            let log_path = std::path::PathBuf::from(crate::arch::execution::paths::LOG_FILE);
            if let Err(e) = crate::arch::logging::upload_logs(&log_path) {
                eprintln!("Failed to upload logs: {}", e);
            }
        }
    }

    exec_result?;

    // 3. Finished
    Box::pin(handle_arch_command(ArchCommands::Finished, debug)).await?;

    Ok(())
}
