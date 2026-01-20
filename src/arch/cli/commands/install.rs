use anyhow::Result;
use colored::Colorize;

use crate::arch::cli::{ArchCommands, DEFAULT_QUESTIONS_FILE};

use super::handle_arch_command;

/// Handle the Install command - orchestrates the full installation process
pub(super) async fn handle_install_command(debug: bool) -> Result<()> {
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
    Box::pin(handle_arch_command(
        ArchCommands::Ask {
            id: None,
            output_config: None,
        },
        debug,
    ))
    .await?;

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
