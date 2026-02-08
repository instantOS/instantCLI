mod ask;
mod dualboot;
mod exec;
mod finished;
mod info;
mod install;
mod setup;
mod upload_logs;

use anyhow::Result;

use crate::arch::cli::{ArchCommands, DualbootCommands};
use crate::arch::engine::Question;
use crate::common::distro::OperatingSystem;

use self::ask::handle_ask_command;
use self::dualboot::handle_dualboot_info;
use self::exec::handle_exec_command;
use self::finished::handle_finished_command;
use self::info::handle_info_command;
use self::install::handle_install_command;
use self::setup::handle_setup_command;
use self::upload_logs::handle_upload_logs;

pub async fn handle_arch_command(command: ArchCommands, debug: bool) -> Result<()> {
    let os = OperatingSystem::detect();

    // Only warn about non-Arch distros for commands other than Info
    if !os.in_family(&OperatingSystem::Arch) && !matches!(command, ArchCommands::Info) {
        eprintln!(
            "Warning: You appear to be running on {}, but this command is intended for Arch Linux.",
            os
        );
    }

    let questions = build_questions();

    match command {
        ArchCommands::List => {
            println!("Available questions:");
            for question in questions {
                println!("- {:?}", question.id());
            }
            Ok(())
        }
        ArchCommands::Ask { id, output_config } => {
            handle_ask_command(id, output_config, questions).await
        }
        ArchCommands::Install => handle_install_command(debug).await,
        ArchCommands::Exec {
            step,
            questions_file,
            dry_run,
        } => handle_exec_command(step, questions_file, dry_run).await,
        ArchCommands::UploadLogs { path } => handle_upload_logs(path),
        ArchCommands::Info => handle_info_command(),
        ArchCommands::Dualboot { command } => match command {
            DualbootCommands::Info => handle_dualboot_info().await,
        },
        ArchCommands::Finished => handle_finished_command().await,
        ArchCommands::Setup { user, dry_run } => handle_setup_command(user, dry_run).await,
    }
}

fn build_questions() -> Vec<Box<dyn Question>> {
    use crate::arch::questions::{
        BooleanQuestion, DiskQuestion, DualBootEspWarning, DualBootPartitionQuestion,
        DualBootSizeQuestion, EncryptionPasswordQuestion, EspPartitionValidator, HostnameQuestion,
        KernelQuestion, KeymapQuestion, LocaleQuestion, MirrorRegionQuestion,
        PartitionSelectorQuestion, PartitioningMethodQuestion, PasswordQuestion,
        ResizeInstructionsQuestion, RunCfdiskQuestion, TimezoneQuestion, UsernameQuestion,
        VirtualBoxWarning, WeakPasswordWarning,
    };

    vec![
        Box::new(VirtualBoxWarning),
        Box::new(crate::arch::questions::warnings::LowRamWarning),
        Box::new(KeymapQuestion),
        Box::new(DiskQuestion),
        Box::new(PartitioningMethodQuestion),
        Box::new(RunCfdiskQuestion),
        Box::new(DualBootPartitionQuestion),
        Box::new(DualBootSizeQuestion),
        Box::new(DualBootEspWarning),
        Box::new(ResizeInstructionsQuestion),
        Box::new(PartitionSelectorQuestion::new(
            crate::arch::engine::QuestionId::RootPartition,
            "Select Root Partition",
            crate::ui::nerd_font::NerdFont::HardDrive,
            None,
        )),
        Box::new(PartitionSelectorQuestion::new(
            crate::arch::engine::QuestionId::BootPartition,
            "Select Boot/EFI Partition",
            crate::ui::nerd_font::NerdFont::Folder,
            Some(Box::new(EspPartitionValidator)),
        )),
        Box::new(
            PartitionSelectorQuestion::new(
                crate::arch::engine::QuestionId::SwapPartition,
                "Select Swap Partition",
                crate::ui::nerd_font::NerdFont::File,
                None,
            )
            .optional(),
        ),
        Box::new(
            PartitionSelectorQuestion::new(
                crate::arch::engine::QuestionId::HomePartition,
                "Select Home Partition",
                crate::ui::nerd_font::NerdFont::Home,
                None,
            )
            .optional(),
        ),
        Box::new(HostnameQuestion),
        Box::new(UsernameQuestion),
        Box::new(PasswordQuestion),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::UseEncryption,
                "Encrypt the installation disk?",
                crate::ui::nerd_font::NerdFont::Lock,
            )
            .dynamic_default(|context| {
                // Encryption features are only available for automatic partitioning
                // If manual partitioning is selected, encryption is not supported
                context
                    .get_answer(&crate::arch::engine::QuestionId::PartitioningMethod)
                    .map(|method| !method.contains("Manual"))
                    .unwrap_or(false)
            })
            .should_ask(|context| {
                // Only ask about encryption if automatic partitioning is selected
                context
                    .get_answer(&crate::arch::engine::QuestionId::PartitioningMethod)
                    .map(|method| !method.contains("Manual"))
                    .unwrap_or(true) // Default to true if partitioning method not yet answered
            }),
        ),
        Box::new(EncryptionPasswordQuestion),
        Box::new(WeakPasswordWarning),
        Box::new(MirrorRegionQuestion),
        Box::new(TimezoneQuestion),
        Box::new(LocaleQuestion),
        Box::new(KernelQuestion),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::UsePlymouth,
                "Enable Plymouth boot splash screen?",
                crate::ui::nerd_font::NerdFont::Monitor,
            )
            .optional()
            .default_yes(),
        ),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::Autologin,
                "Enable LightDM Autologin?",
                crate::ui::nerd_font::NerdFont::User,
            )
            .optional()
            .dynamic_default(|context| {
                context.get_answer_bool(crate::arch::engine::QuestionId::UseEncryption)
            }),
        ),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::LogUpload,
                "Upload installation logs to snips.sh?",
                crate::ui::nerd_font::NerdFont::Debug,
            )
            .optional()
            .default_yes(),
        ),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::QuestionId::MinimalMode,
                "Enable Minimal Mode (Vanilla Arch Install)?",
                crate::ui::nerd_font::NerdFont::Package,
            )
            .optional(),
        ),
    ]
}
