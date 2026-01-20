use anyhow::Result;
use colored::Colorize;

use crate::arch::dualboot::{analyze_all_disks, display_disks};
use crate::ui::nerd_font::NerdFont;

/// Handle dual boot info display
pub(super) async fn handle_dualboot_info() -> Result<()> {
    println!();
    println!(
        "  {} {}",
        NerdFont::HardDrive.to_string().bright_cyan(),
        "Dual Boot Detection".bright_white().bold()
    );
    println!("  {}", "─".repeat(50).bright_black());
    println!();

    match analyze_all_disks() {
        Ok(analyses) => {
            let mut any_feasible = false;

            // Show feasibility summary first
            println!("  {}", "Feasibility Summary:".bold());
            for analysis in &analyses {
                let disk = &analysis.disk.device;
                let feasibility = &analysis.feasibility;
                if feasibility.feasible {
                    any_feasible = true;
                    // Show different message depending on whether feasibility
                    // comes from resizable partitions or unpartitioned space
                    if feasibility.feasible_partitions.is_empty() {
                        // Feasible due to unpartitioned space
                        println!(
                            "    {} {} - {}",
                            NerdFont::Check.to_string().green().bold(),
                            disk.bright_white(),
                            "FEASIBLE".green().bold(),
                        );
                        if let Some(reason) = &feasibility.reason {
                            println!(
                                "      {} {}",
                                NerdFont::ArrowPointer.to_string().dimmed(),
                                reason.green()
                            );
                        }
                    } else {
                        // Feasible due to resizable partitions
                        println!(
                            "    {} {} - {} {}",
                            NerdFont::Check.to_string().green().bold(),
                            disk.bright_white(),
                            "FEASIBLE".green().bold(),
                            format!(
                                "({} partition(s) can be resized)",
                                feasibility.feasible_partitions.len()
                            )
                            .dimmed()
                        );
                        for part in &feasibility.feasible_partitions {
                            println!(
                                "      {} {}",
                                NerdFont::ArrowPointer.to_string().dimmed(),
                                part.cyan()
                            );
                        }
                    }
                } else {
                    println!(
                        "    {} {} - {}",
                        NerdFont::Cross.to_string().red().bold(),
                        disk.bright_white(),
                        "NOT FEASIBLE".red().bold()
                    );
                    if let Some(reason) = &feasibility.reason {
                        println!(
                            "      {} {}",
                            NerdFont::ArrowPointer.to_string().dimmed(),
                            reason.dimmed()
                        );
                    }
                }
            }

            println!();

            if any_feasible {
                println!(
                    "  {} {}",
                    NerdFont::Check.to_string().green().bold(),
                    "Dual boot is POSSIBLE on this system".green().bold()
                );
                println!(
                    "  {} The installer will offer dual boot options",
                    NerdFont::ArrowPointer.to_string().dimmed()
                );
            } else {
                println!(
                    "  {} {}",
                    NerdFont::Cross.to_string().red().bold(),
                    "Dual boot is NOT POSSIBLE on this system".red().bold()
                );
                println!(
                    "  {} The installer will NOT offer dual boot options",
                    NerdFont::ArrowPointer.to_string().dimmed()
                );
            }

            println!();
            println!("  {}", "Detailed Disk Information:".bold());
            println!("  {}", "─".repeat(50).bright_black());
            println!();

            // Show detailed disk information
            if analyses.is_empty() {
                println!(
                    "  {} No disks detected. Are you running as root?",
                    NerdFont::Warning.to_string().yellow()
                );
            } else {
                let disks: Vec<_> = analyses.iter().map(|a| &a.disk).collect();
                display_disks(&disks);
            }
        }
        Err(e) => {
            eprintln!(
                "  {} Failed to check feasibility: {}",
                NerdFont::Cross.to_string().red(),
                e
            );
            eprintln!(
                "  {} Try running with sudo",
                NerdFont::ArrowPointer.to_string().dimmed()
            );
        }
    }

    Ok(())
}
