use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum ArchCommands {
    /// Start the Arch Linux installation wizard
    Install,
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
            let question = questions
                .into_iter()
                .find(|q| q.id() == id)
                .ok_or_else(|| anyhow::anyhow!("Question not found"))?;

            // For single question asking, we might need a dummy context or partial context
            // depending on dependencies. For now, we'll create a default context.
            // Note: If the question depends on async data (like MirrorRegion), this might fail
            // or need the background task.

            let mut engine = QuestionEngine::new(vec![]); // Dummy engine to get context

            // Spawn background task to fetch data (same as install)
            // This is needed for questions like MirrorRegion/Timezone
            let data_clone = engine.context.data.clone();
            tokio::spawn(async move {
                // Simulate network delay for mirrors
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                {
                    let mut data = data_clone.lock().unwrap();
                    // In reality: fetch from archlinux.org/mirrorlist
                    data.insert(
                        "mirror_regions".to_string(),
                        "Germany,United States,France,Japan".to_string(),
                    );
                }

                // Simulate filesystem scan for timezones
                // In reality: walkdir /usr/share/zoneinfo
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                {
                    let mut data = data_clone.lock().unwrap();
                    data.insert(
                        "timezones".to_string(),
                        "Europe/Berlin\nEurope/London\nAmerica/New_York".to_string(),
                    );
                }
            });

            // Wait for readiness if needed
            while !question.is_ready(&engine.context) {
                println!("Waiting for data...");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }

            loop {
                let answer = question.ask(&engine.context).await?;
                match question.validate(&answer) {
                    Ok(()) => {
                        println!("Answer: {}", answer);
                        break;
                    }
                    Err(msg) => {
                        crate::menu_utils::FzfWrapper::message(&msg)?;
                    }
                }
            }
            Ok(())
        }
        ArchCommands::Install => {
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
            println!("  Boot Mode: {:?}", system_info.boot_mode);
            println!("  Internet: {}", system_info.internet_connected);
            println!("  AMD CPU: {}", system_info.has_amd_cpu);
            println!("  Intel CPU: {}", system_info.has_intel_cpu);
            println!("  NVIDIA GPU: {}", system_info.has_nvidia_gpu);

            let mut engine = QuestionEngine::new(questions);
            engine.context.system_info = system_info;

            // Spawn background task to fetch data
            let data_clone = engine.context.data.clone();
            tokio::spawn(async move {
                // Simulate network delay for mirrors
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                {
                    let mut data = data_clone.lock().unwrap();
                    // In reality: fetch from archlinux.org/mirrorlist
                    data.insert(
                        "mirror_regions".to_string(),
                        "Germany,United States,France,Japan".to_string(),
                    );
                }

                // Simulate filesystem scan for timezones
                // In reality: walkdir /usr/share/zoneinfo
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                {
                    let mut data = data_clone.lock().unwrap();
                    data.insert(
                        "timezones".to_string(),
                        "Europe/Berlin\nEurope/London\nAmerica/New_York".to_string(),
                    );
                }
            });

            let context = engine.run().await?;

            println!("Installation configuration complete!");
            println!(
                "Hostname: {:?}",
                context.get_answer(&crate::arch::engine::QuestionId::Hostname)
            );
            println!(
                "Username: {:?}",
                context.get_answer(&crate::arch::engine::QuestionId::Username)
            );

            println!("\n--- Configuration TOML ---");
            println!("{}", context.to_toml()?);
            println!("--------------------------");

            Ok(())
        }
    }
}
