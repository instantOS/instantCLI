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
use crate::arch::engine::WizardStep;
use crate::common::distro::OperatingSystem;

use self::ask::{AskOutcome, handle_ask_command};
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

    let steps = build_steps();

    match command {
        ArchCommands::List => {
            println!("Available wizard steps:");
            for step in steps {
                println!("- {:?}", step.id());
            }
            Ok(())
        }
        ArchCommands::Ask { id, output_config } => {
            match handle_ask_command(id, output_config, steps).await? {
                AskOutcome::Completed | AskOutcome::Cancelled => Ok(()),
            }
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

pub(super) fn build_steps() -> Vec<Box<dyn WizardStep>> {
    use crate::arch::questions::{
        BooleanQuestion, DesktopEnvironmentQuestion, DiskQuestion, DualBootEspWarning,
        DualBootPartitionQuestion, DualBootSizeQuestion, EncryptionPasswordQuestion,
        EspPartitionValidator, KernelQuestion, KeymapQuestion, LocaleQuestion,
        MirrorRegionQuestion, PartitionSelectorQuestion, PartitioningMethodQuestion,
        PasswordQuestion, PrepareDiskStep, ResizeWorkflowStep, RunCfdiskStep, TimezoneQuestion,
        VirtualBoxWarning, WeakPasswordWarning, hostname_question, username_question,
    };
    use crate::arch::questions::{
        BtrfsCompressionQuestion, DisplayManagerQuestion, RootFilesystemQuestion,
    };

    vec![
        Box::new(VirtualBoxWarning),
        Box::new(crate::arch::questions::warnings::LowRamWarning),
        Box::new(KeymapQuestion),
        Box::new(DiskQuestion),
        Box::new(PrepareDiskStep),
        Box::new(PartitioningMethodQuestion),
        Box::new(RunCfdiskStep),
        Box::new(DualBootPartitionQuestion),
        Box::new(DualBootSizeQuestion),
        Box::new(DualBootEspWarning),
        Box::new(ResizeWorkflowStep),
        Box::new(PartitionSelectorQuestion::new(
            crate::arch::engine::StepId::RootPartition,
            "Select Root Partition",
            crate::ui::nerd_font::NerdFont::HardDrive,
            None,
        )),
        Box::new(PartitionSelectorQuestion::new(
            crate::arch::engine::StepId::BootPartition,
            "Select Boot/EFI Partition",
            crate::ui::nerd_font::NerdFont::Folder,
            Some(Box::new(EspPartitionValidator)),
        )),
        Box::new(
            PartitionSelectorQuestion::new(
                crate::arch::engine::StepId::SwapPartition,
                "Select Swap Partition",
                crate::ui::nerd_font::NerdFont::File,
                None,
            )
            .optional(),
        ),
        Box::new(
            PartitionSelectorQuestion::new(
                crate::arch::engine::StepId::HomePartition,
                "Select Home Partition",
                crate::ui::nerd_font::NerdFont::Home,
                None,
            )
            .optional(),
        ),
        Box::new(hostname_question()),
        Box::new(username_question()),
        Box::new(PasswordQuestion),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::StepId::UseEncryption,
                "Encrypt the installation disk?",
                crate::ui::nerd_font::NerdFont::Lock,
            )
            .description(
                "Protects your data with LUKS full-disk encryption. A passphrase will be required at boot.",
            )
            .default_from(
                [crate::arch::engine::StepId::PartitioningMethod],
                |context| {
                    // Encryption features are only available for automatic partitioning
                    // If manual partitioning is selected, encryption is not supported
                    context
                        .get_answer(&crate::arch::engine::StepId::PartitioningMethod)
                        .map(|method| !method.contains("Manual"))
                        .unwrap_or(false)
                },
            )
            .relevant_when(
                [crate::arch::engine::StepId::PartitioningMethod],
                |context| {
                    // Only ask about encryption if automatic partitioning is selected
                    context
                        .get_answer(&crate::arch::engine::StepId::PartitioningMethod)
                        .map(|method| !method.contains("Manual"))
                        .unwrap_or(true) // Default to true if partitioning method not yet answered
                },
            ),
        ),
        Box::new(EncryptionPasswordQuestion),
        Box::new(WeakPasswordWarning),
        Box::new(MirrorRegionQuestion),
        Box::new(TimezoneQuestion),
        Box::new(LocaleQuestion),
        Box::new(KernelQuestion),
        Box::new(DesktopEnvironmentQuestion),
        Box::new(RootFilesystemQuestion),
        Box::new(BtrfsCompressionQuestion),
        Box::new(DisplayManagerQuestion),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::StepId::UsePlymouth,
                "Enable Plymouth boot splash screen?",
                crate::ui::nerd_font::NerdFont::Monitor,
            )
            .description(
                "Displays a graphical boot animation and hides console kernel logs during system startup.",
            )
            .optional()
            .default_yes(),
        ),
        Box::new(autologin_question(AutologinDefault::MatchEncryption)),
        Box::new(use_xorg_question()),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::StepId::LogUpload,
                "Upload installation logs to snips.sh?",
                crate::ui::nerd_font::NerdFont::Debug,
            )
            .description(
                "Uploads an anonymized installation log to snips.sh upon completion to help troubleshoot setup issues.",
            )
            .optional()
            .default_yes(),
        ),
        Box::new(
            BooleanQuestion::new(
                crate::arch::engine::StepId::MinimalMode,
                "Enable Minimal Mode (Vanilla Arch Install)?",
                crate::ui::nerd_font::NerdFont::Package,
            )
            .description(
                "Installs a clean, vanilla Arch Linux system skipping instantOS packages, default apps, and custom theming.",
            )
            .optional(),
        ),
    ]
}

pub(super) enum AutologinDefault {
    Disabled,
    MatchEncryption,
}

pub(super) fn autologin_question(
    default: AutologinDefault,
) -> crate::arch::questions::BooleanQuestion {
    use crate::arch::engine::StepId;

    let question = crate::arch::questions::BooleanQuestion::new(
        StepId::Autologin,
        "Enable Display Manager Autologin?",
        crate::ui::nerd_font::NerdFont::User,
    )
    .description(
        "Automatically logs into your user account on boot without prompting for a password at the display manager.",
    )
    .optional()
    .relevant_when([StepId::DesktopEnvironment], |context| {
        crate::arch::config::DesktopEnvironment::from_context(context).requires_display_manager()
    });

    match default {
        AutologinDefault::MatchEncryption => question
            .default_from([StepId::UseEncryption], |context| {
                context.get_answer_bool(StepId::UseEncryption)
            }),
        AutologinDefault::Disabled => question,
    }
}

pub(super) fn use_xorg_question() -> crate::arch::questions::BooleanQuestion {
    use crate::arch::engine::StepId;

    crate::arch::questions::BooleanQuestion::new(
        StepId::UseXorg,
        "Install Xorg server (enable X11 session)?",
        crate::ui::nerd_font::NerdFont::Desktop,
    )
    .description(
        "instantWM supports both X11 and Wayland sessions. By default, instantOS uses Wayland.\n\
         Enabling this option installs xorg-server so the classic X11 instantWM session is available in your display manager.",
    )
    .optional()
    .relevant_when([StepId::DesktopEnvironment], |context| {
        crate::arch::config::DesktopEnvironment::from_context(context).requires_display_manager()
    })
    .default_from([StepId::DisplayManager], |context| {
        crate::arch::config::DisplayManager::from_context(context)
            == crate::arch::config::DisplayManager::Lightdm
    })
}

#[cfg(test)]
mod tests {
    use super::build_steps;
    use crate::arch::engine::WizardEngine;

    #[test]
    fn install_question_graph_is_valid() {
        WizardEngine::new(build_steps()).unwrap();
    }
}
