use anyhow::Result;
use colored::Colorize;

use crate::arch::cli::DEFAULT_QUESTIONS_FILE;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;

/// Handle the installation finished menu
pub(super) async fn handle_finished_command() -> Result<()> {
    #[derive(Clone)]
    enum FinishedMenuOption {
        Reboot,
        Shutdown,
        Continue,
    }

    impl FzfSelectable for FinishedMenuOption {
        fn fzf_display_text(&self) -> String {
            match self {
                FinishedMenuOption::Reboot => format!("{} Reboot", NerdFont::Reboot),
                FinishedMenuOption::Shutdown => format!("{} Shutdown", NerdFont::PowerOff),
                FinishedMenuOption::Continue => {
                    format!("{} Continue in Live Session", NerdFont::Continue)
                }
            }
        }
    }

    let state = crate::arch::execution::state::InstallState::load()?;

    // Check if we should upload logs
    if let Ok(context) = crate::arch::engine::InstallContext::load(DEFAULT_QUESTIONS_FILE) {
        crate::arch::logging::process_log_upload(&context);
    }

    println!("\n{}", "Installation Finished!".green().bold());

    if let Some(start_time) = state.start_time {
        let duration = chrono::Utc::now() - start_time;
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;
        let seconds = duration.num_seconds() % 60;
        println!("Duration: {:02}:{:02}:{:02}", hours, minutes, seconds);
    }

    // Calculate storage used (approximate)
    if let Ok(output) = std::process::Command::new("df")
        .arg("-h")
        .arg("/mnt")
        .output()
    {
        let output_str = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = output_str.lines().nth(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                println!("Storage Used: {}", parts[2]);
            }
        }
    }

    println!();

    let options = vec![
        FinishedMenuOption::Reboot,
        FinishedMenuOption::Shutdown,
        FinishedMenuOption::Continue,
    ];

    let result = FzfWrapper::builder()
        .header("Installation complete. What would you like to do?")
        .select(options)?;

    match result {
        FzfResult::Selected(option) => match option {
            FinishedMenuOption::Reboot => {
                println!("Rebooting...");
                std::process::Command::new("reboot").spawn()?;
            }
            FinishedMenuOption::Shutdown => {
                println!("Shutting down...");
                std::process::Command::new("poweroff").spawn()?;
            }
            FinishedMenuOption::Continue => {
                println!("Exiting to live session...");
            }
        },
        _ => println!("Exiting..."),
    }

    Ok(())
}
