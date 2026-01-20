use anyhow::Result;
use colored::{ColoredString, Colorize};

use crate::arch::engine::SystemInfo;
use crate::ui::nerd_font::NerdFont;

pub fn detect_single_user() -> Option<String> {
    let home = std::path::Path::new("/home");
    if !home.exists() {
        return None;
    }

    let entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(_) => return None,
    };

    let mut users = Vec::new();
    for entry in entries.flatten() {
        if let Ok(file_type) = entry.file_type()
            && file_type.is_dir()
            && let Ok(name) = entry.file_name().into_string()
            && name != "lost+found"
        {
            users.push(name);
        }
    }

    if users.len() == 1 {
        Some(users[0].clone())
    } else {
        None
    }
}

pub fn ensure_root() -> Result<()> {
    if let sudo::RunningAs::User = sudo::check() {
        sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"])
            .map_err(|e| anyhow::anyhow!("Failed to escalate privileges: {}", e))?;
    }
    Ok(())
}

pub fn print_system_info(info: &SystemInfo) {
    let print_row = |icon: ColoredString, label: &str, value: &dyn std::fmt::Display| {
        println!("  {}   {:<20} {}", icon, label, value);
    };

    println!();
    println!(
        "  {} {}",
        NerdFont::Desktop.to_string().bright_cyan(),
        "System Information".bright_white().bold()
    );
    println!("  {}", "─".repeat(50).bright_black());

    // Distro
    print_row(
        NerdFont::Terminal.to_string().bright_cyan(),
        "Distro:",
        &info.distro.bright_cyan(),
    );

    // Boot Mode
    let boot_mode_str = match info.boot_mode {
        crate::arch::engine::BootMode::UEFI64 => "UEFI 64-bit",
        crate::arch::engine::BootMode::UEFI32 => "UEFI 32-bit",
        crate::arch::engine::BootMode::BIOS => "BIOS",
    };
    print_row(
        NerdFont::PowerOff.to_string().bright_green(),
        "Boot Mode:",
        &boot_mode_str.bright_green(),
    );

    // Architecture
    print_row(
        NerdFont::Cpu.to_string().bright_magenta(),
        "Architecture:",
        &info.architecture.bright_magenta(),
    );

    // RAM
    if let Some(ram_gb) = info.total_ram_gb {
        let ram_str = format!("{} GB", ram_gb);
        let colored = if ram_gb >= 4 {
            ram_str.bright_green()
        } else if ram_gb >= 1 {
            ram_str.bright_yellow()
        } else {
            ram_str.bright_red()
        };
        print_row(
            NerdFont::Memory.to_string().bright_cyan(),
            "Memory:",
            &colored,
        );
    }

    // CPU
    if info.has_intel_cpu {
        print_row(
            NerdFont::Cpu.to_string().bright_blue(),
            "CPU:",
            &"Intel".bright_blue(),
        );
    } else if info.has_amd_cpu {
        print_row(
            NerdFont::Cpu.to_string().bright_red(),
            "CPU:",
            &"AMD".bright_red(),
        );
    }

    // GPUs
    if !info.gpus.is_empty() {
        let gpu_strs: Vec<String> = info.gpus.iter().map(|gpu| gpu.to_string()).collect();
        let gpu_str = gpu_strs.join(", ");

        let colored_gpu_str = if info.gpus.len() == 1 {
            info.gpus[0].to_colored_string()
        } else {
            gpu_str.normal()
        };

        print_row(
            NerdFont::Gpu.to_string().bright_cyan(),
            "GPU:",
            &colored_gpu_str,
        );
    }

    // Virtualization
    if let Some(vm_type) = &info.vm_type {
        println!(
            "  {}   {:<20} {} ({})",
            NerdFont::Server.to_string().bright_yellow(),
            "Virtualization:",
            vm_type.bright_yellow(),
            "Virtual Machine".bright_black()
        );
    } else {
        print_row(
            NerdFont::Server.to_string().bright_green(),
            "Virtualization:",
            &"Bare Metal".bright_green(),
        );
    }

    // Internet
    if info.internet_connected {
        print_row(
            NerdFont::Globe.to_string().bright_green(),
            "Internet:",
            &"Connected".bright_green(),
        );
    } else {
        print_row(
            NerdFont::Globe.to_string().bright_red(),
            "Internet:",
            &"Disconnected".bright_red(),
        );
    }

    println!("  {}", "─".repeat(50).bright_black());
    println!();
}
