use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum ArchCommands {
    /// Start the Arch Linux installation wizard
    Install {
        /// Optional path to save the configuration TOML file
        #[arg(short = 'o', long)]
        output_config: Option<std::path::PathBuf>,
    },
    /// List all available questions
    List,
    /// Ask a specific question
    Ask {
        /// The ID of the question to ask
        #[arg(value_enum)]
        id: crate::arch::engine::QuestionId,
    },
}

pub async fn handle_arch_command(command: ArchCommands, _debug: bool) -> Result<()> {
    use crate::arch::engine::QuestionEngine;
    use crate::arch::questions::{
        DiskQuestion, HostnameQuestion, KeymapQuestion, LocaleQuestion, MirrorRegionQuestion,
        PasswordQuestion, TimezoneQuestion, UsernameQuestion,
    };

    let questions: Vec<Box<dyn crate::arch::engine::Question>> = vec![
        Box::new(KeymapQuestion),
        Box::new(DiskQuestion),
        Box::new(HostnameQuestion),
        Box::new(UsernameQuestion),
        Box::new(PasswordQuestion),
        Box::new(MirrorRegionQuestion),
        Box::new(TimezoneQuestion),
        Box::new(LocaleQuestion),
    ];

    match command {
        ArchCommands::List => {
            println!("Available questions:");
            for question in questions {
                println!("- {:?}", question.id());
            }
            Ok(())
        }
        ArchCommands::Ask { id } => {
            // Escalate if the question requires root (e.g. Disk)
            if matches!(id, crate::arch::engine::QuestionId::Disk) {
                ensure_root()?;
            }

            let question = questions
                .into_iter()
                .find(|q| q.id() == id)
                .ok_or_else(|| anyhow::anyhow!("Question not found"))?;

            let engine = QuestionEngine::new(vec![question]);

            // Initialize data providers so questions that need data (like MirrorRegion) work
            engine.initialize_providers();

            // Run the engine with just this single question
            // This handles is_ready, validation, cancellation, etc.
            let context = engine.run().await?;

            if let Some(answer) = context.get_answer(&id) {
                println!("Answer: {}", answer);
            }
            Ok(())
        }
        ArchCommands::Install { output_config } => {
            // Installation requires root privileges
            ensure_root()?;

            println!("Starting Arch Linux installation wizard...");

            // Perform system checks
            let mut system_info = crate::arch::engine::SystemInfo::default();

            // Internet check
            system_info.internet_connected = crate::settings::network::check_internet();
            if !system_info.internet_connected {
                eprintln!(
                    "Error: No internet connection detected. Arch installation requires internet."
                );
                return Ok(());
            }

            // Boot mode check
            if std::path::Path::new("/sys/firmware/efi/fw_platform_size").exists() {
                let content = std::fs::read_to_string("/sys/firmware/efi/fw_platform_size")
                    .unwrap_or_default();
                if content.trim() == "64" {
                    system_info.boot_mode = crate::arch::engine::BootMode::UEFI64;
                } else if content.trim() == "32" {
                    system_info.boot_mode = crate::arch::engine::BootMode::UEFI32;
                }
            } else if std::path::Path::new("/sys/firmware/efi").exists() {
                // Fallback if fw_platform_size doesn't exist but efi does
                system_info.boot_mode = crate::arch::engine::BootMode::UEFI64;
            }

            // CPU check
            if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
                system_info.has_amd_cpu = cpuinfo.contains("AuthenticAMD");
                system_info.has_intel_cpu = cpuinfo.contains("GenuineIntel");
            }

            // GPU check (simple lspci check)
            // In a real app we might use a crate or more robust command
            if let Ok(lspci) = std::process::Command::new("lspci").output() {
                let output = String::from_utf8_lossy(&lspci.stdout);
                system_info.has_nvidia_gpu = output.to_lowercase().contains("nvidia");
            }

            println!("System Checks:");
            let boot_mode_str = match system_info.boot_mode {
                crate::arch::engine::BootMode::UEFI64 => "UEFI64",
                crate::arch::engine::BootMode::UEFI32 => "UEFI32",
                crate::arch::engine::BootMode::BIOS => "BIOS",
            };
            println!("  Boot Mode: {}", boot_mode_str);
            println!("  Internet: {}", system_info.internet_connected);
            println!("  AMD CPU: {}", system_info.has_amd_cpu);
            println!("  Intel CPU: {}", system_info.has_intel_cpu);
            println!("  NVIDIA GPU: {}", system_info.has_nvidia_gpu);

            let mut engine = QuestionEngine::new(questions);
            engine.context.system_info = system_info;

            // Initialize data providers
            engine.initialize_providers();

            let context = engine.run().await?;

            println!("Installation configuration complete!");
            println!(
                "Hostname: {}",
                context.get_answer(&crate::arch::engine::QuestionId::Hostname)
                    .map_or("<not set>".to_string(), |v| v.clone())
            );
            println!(
                "Username: {}",
                context.get_answer(&crate::arch::engine::QuestionId::Username)
                    .map_or("<not set>".to_string(), |v| v.clone())
            );

            let toml_content = context.to_toml()?;

            if let Some(config_path) = output_config {
                // Write to file
                std::fs::write(&config_path, &toml_content)?;
                println!("\nConfiguration saved to: {}", config_path.display());
            } else {
                // Print to stdout
                println!("\n--- Configuration TOML ---");
                println!("{}", toml_content);
                println!("--------------------------");
            }

            Ok(())
        }
    }
}

fn ensure_root() -> Result<()> {
    if let sudo::RunningAs::User = sudo::check() {
        sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"])
            .map_err(|e| anyhow::anyhow!("Failed to escalate privileges: {}", e))?;
    }
    Ok(())
}
