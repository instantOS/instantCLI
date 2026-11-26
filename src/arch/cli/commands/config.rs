use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;

use crate::arch::cli::ConfigCommands;
use crate::arch::config::{BtrfsCompression, DesktopEnvironment, DisplayManager, RootFilesystem};
use crate::arch::engine::{
    DualBootTarget, FREE_SPACE_ANSWER, InstallContext, InstallPlan, Kernel, PartitioningMethod,
    StepId, StoragePlan, SystemInfo, WizardStep, partition_action,
};

pub fn print_config_guide() {
    print!(
        "# Write and check an ins arch install config

1. On the machine that will run the installer, generate a starter file:
   `(umask 077; ins arch config example > questions.toml)`
2. Edit the `[answers]` table. Set the disk, credentials, and other choices.
   Automatic partitioning erases the selected disk. Store credentials privately.
3. Run `ins arch config explain` for the field list, or
   `ins arch config explain PartitioningMethod` for storage options.
4. Run `ins arch check -f questions.toml` to validate the file and preview the
   plan. Add `--target` on the install host to require the named block devices
   to exist. Review the disk and storage operation before authorizing installation.

Answer keys are case-sensitive and values are TOML strings, including
`\"yes\"` and `\"no\"`. Keep the generated `[system_info]` table for the
intended install host. If you edit a wizard export, remove its
`step_dependency_fingerprints` table. Hand-authored files need no wizard metadata.

Run `ins arch dualboot info --output json` on the install host to discover disks,
existing OS hints, and ways to make room for Linux.

Run `sudo ins arch exec -f questions.toml` only after the installation and its
target have been approved.
"
    );
}

pub(super) fn handle_config_command(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Example => {
            let mut system_info = SystemInfo::detect_with_network(false);
            system_info.gpus.sort_by_key(|gpu| gpu.to_string());
            println!(
                r#"# Replace the empty disk path and password, then run ins arch check.
[answers]
# Automatic partitioning erases the selected whole disk.
Disk = ""
PartitioningMethod = "automatic"
UseEncryption = "no"
Hostname = "arch-host"
Username = "installer"
Password = ""
Keymap = "us"
Timezone = "UTC"
Locale = "en_US.UTF-8"

# Hardware detected on the machine that generated this file.
[system_info]"#
            );
            print!("{}", toml::to_string_pretty(&system_info)?);
        }
        ConfigCommands::Explain { field: None } => {
            println!("Use `ins arch config explain FIELD` for values and requirements.\n");
            println!("[answers]");
            for &id in StepId::value_variants() {
                if field_help(id).is_some() {
                    println!("  {id:?}  {}", id.display_name());
                }
            }
            println!("\n[system_info]  Hardware facts for the install host");
        }
        ConfigCommands::Explain { field: Some(field) } => {
            if matches!(field.as_str(), "system_info" | "system-info") {
                println!(
                    "[system_info]\nGenerate this table on the install host with \
                     `ins arch config example`. Its boot mode selects the partition layout and \
                     bootloader setup. CPU, GPU, and other device facts affect package selection."
                );
                return Ok(());
            }
            for &id in StepId::value_variants() {
                if (field == format!("{id:?}")
                    || id
                        .to_possible_value()
                        .is_some_and(|value| field == value.get_name()))
                    && let Some(help) = field_help(id)
                {
                    println!("{id:?}\n{help}");
                    return Ok(());
                }
            }
            bail!("Unknown install config field {field:?}. See `ins arch config explain`.");
        }
    }
    Ok(())
}

fn field_help(id: StepId) -> Option<String> {
    let help = match id {
        StepId::Hostname => "Required. Hostname for the installed system.",
        StepId::Username => "Required. Name of the first regular user.",
        StepId::Password => {
            "Required. Plaintext password for both root and the first user. Keep this file private."
        }
        StepId::Keymap => "Required. Linux console keymap name, for example `us` or `de-latin1`.",
        StepId::ConsoleFont => {
            return Some(format!(
                "Optional. Console font name from the install host. Default `{}`.",
                crate::common::console_font::DEFAULT_NAME
            ));
        }
        StepId::Disk => {
            "Required. Whole block device path under `/dev`. Use `ins arch check --target -f FILE` on the install host to check that it exists."
        }
        StepId::PartitioningMethod => {
            return Some(format!(
                "Required.\n- `{}` erases and repartitions the whole disk. Set UseEncryption.\n\
                 - `{}` uses free space or resizes a partition. Set DualBootPartition.\n\
                 - `{}` formats selected partitions. Set RootPartition and BootPartition.",
                PartitioningMethod::Automatic.answer_value(),
                PartitioningMethod::DualBoot.answer_value(),
                PartitioningMethod::Manual.answer_value(),
            ));
        }
        StepId::UseEncryption => {
            "Required with automatic partitioning. Use `yes` for LUKS encryption or `no`. Omit with other partitioning methods."
        }
        StepId::EncryptionPassword => {
            "Required when UseEncryption is `yes`. Plaintext LUKS passphrase. Keep this file private."
        }
        StepId::RootPartition => {
            "Required with manual partitioning. Path of the partition to format and mount at `/`."
        }
        StepId::BootPartition => {
            "Required with manual partitioning. Path of the partition to format and mount at `/boot`."
        }
        StepId::SwapPartition => {
            "Optional with manual partitioning. Path of the partition to initialize as swap."
        }
        StepId::HomePartition => {
            "Optional with manual partitioning. Use `root` (or omit) to use space on root, `keep:/dev/...` to mount an existing ext2, ext3, ext4, or XFS partition, or a plain `/dev/...` path to erase and format it as ext4."
        }
        StepId::DualBootPartition => {
            return Some(format!(
                "Required with dual-boot partitioning. Use `{FREE_SPACE_ANSWER}` \
                 or the path of a partition on Disk. A partition path requires DualBootInstructions."
            ));
        }
        StepId::DualBootInstructions => {
            "Required when DualBootPartition names a partition. Use `auto` to resize it during installation, or `confirmed` if you already resized it."
        }
        StepId::DualBootSize => {
            "Required for automatic dual-boot resizing. Desired free space as a quoted positive byte count."
        }
        StepId::Timezone => {
            "Required. Name from `/usr/share/zoneinfo`, for example `UTC` or `Europe/Berlin`."
        }
        StepId::Locale => {
            "Required. Locale listed in the install environment's `/etc/locale.gen`, for example `en_US.UTF-8`."
        }
        StepId::MirrorRegion => {
            "Optional. Arch mirror region name from the wizard. Omit to use the default mirror configuration. Ignored on Artix."
        }
        StepId::Kernel => {
            return Some(choices(
                &Kernel::ALL.map(|kernel| kernel.label()),
                Kernel::Linux.label(),
            ));
        }
        StepId::DesktopEnvironment => {
            return Some(choices(
                &DesktopEnvironment::ALL.map(|desktop| desktop.answer_value()),
                DesktopEnvironment::DEFAULT.answer_value(),
            ));
        }
        StepId::RootFilesystem => {
            return Some(choices(
                &RootFilesystem::ALL.map(|filesystem| filesystem.answer_value()),
                RootFilesystem::DEFAULT.answer_value(),
            ));
        }
        StepId::BtrfsCompression => {
            return Some(format!(
                "Only applies to Btrfs. {}",
                choices(
                    &BtrfsCompression::ALL.map(|level| level.answer_value()),
                    BtrfsCompression::DEFAULT.answer_value(),
                )
            ));
        }
        StepId::DisplayManager => {
            return Some(format!(
                "Omit for DesktopEnvironment `none/tty`. {}",
                choices(
                    &DisplayManager::ALL.map(|manager| manager.answer_value()),
                    DisplayManager::DEFAULT.answer_value(),
                )
            ));
        }
        StepId::UsePlymouth => {
            "Optional. Use `yes` or `no`. Defaults to `yes` where Plymouth is supported. Artix skips Plymouth."
        }
        StepId::Autologin => {
            "Optional. Use `yes` or `no`. Defaults to `yes` with automatic encryption and `no` otherwise. Omit for DesktopEnvironment `none/tty`."
        }
        StepId::UseXorg => {
            "Optional. Use `yes` or `no`. Defaults to `yes` with LightDM and `no` otherwise. Omit for DesktopEnvironment `none/tty`."
        }
        StepId::MinimalMode => {
            "Optional. Use `yes` for a vanilla install or `no` for instantOS. Default `no`."
        }
        StepId::LogUpload
        | StepId::ConfirmInstall
        | StepId::RunCfdisk
        | StepId::VirtualBoxWarning
        | StepId::WeakPasswordWarning
        | StepId::LowRamWarning
        | StepId::DualBootEspWarning => return None,
    };
    Some(help.to_owned())
}

fn choices(values: &[&str], default: &str) -> String {
    format!(
        "Optional. Values `{}`. Default `{default}`.",
        values.join("`, `")
    )
}

pub(super) fn check_config(steps: &[Box<dyn WizardStep>], path: &Path, target: bool) -> Result<()> {
    let source = fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    let context = InstallContext::parse_config(&source, path)?;
    let table: toml::Table = toml::from_str(&source).context("Reading configuration tables")?;
    for key in table.keys() {
        ensure!(
            matches!(
                key.as_str(),
                "answers" | "system_info" | "completed_steps" | "step_dependency_fingerprints"
            ),
            "Unknown top-level config field {key:?} in {}",
            path.display()
        );
    }
    let plan = InstallPlan::from_imported_config(steps, &context)?;
    let disk = plan.storage().disk().as_str();
    if target {
        require_block_device(disk)?;
    }
    let operation = match plan.storage() {
        StoragePlan::Automatic { encryption, .. } => {
            if encryption.is_some() {
                "Erase and repartition the whole disk with LUKS encryption".to_owned()
            } else {
                "Erase and repartition the whole disk".to_owned()
            }
        }
        StoragePlan::Manual { partitions, .. } => {
            if target {
                for device in [&partitions.root, &partitions.boot]
                    .into_iter()
                    .chain(partitions.swap.iter())
                    .chain(partitions.home.iter().map(|home| home.path()))
                {
                    require_block_device(device.as_str())?;
                }
                crate::arch::execution::disk::validate_kept_home_devices(partitions)?;
            }
            let mut actions = vec![
                partition_action(partitions.root.as_str(), "format as /"),
                partition_action(partitions.boot.as_str(), "format as /boot"),
            ];
            if let Some(swap) = &partitions.swap {
                actions.push(partition_action(swap.as_str(), "initialize as swap"));
            }
            actions.push(partitions.home.as_ref().map_or_else(
                || "/home → use space on root".to_owned(),
                |home| home.description(),
            ));
            actions.join("\n")
        }
        StoragePlan::DualBoot {
            target: dual_boot, ..
        } => match dual_boot {
            DualBootTarget::ExistingFreeSpace => "Install into existing free space".to_owned(),
            DualBootTarget::ResizeAutomatically {
                partition,
                desired_free_space_bytes,
            } => {
                if target {
                    require_block_device(partition.as_str())?;
                }
                format!(
                    "Resize {} to leave {desired_free_space_bytes} bytes of free space",
                    partition.as_str()
                )
            }
            DualBootTarget::ResizedManually { partition } => {
                if target {
                    require_block_device(partition.as_str())?;
                }
                format!(
                    "Install into the free space left after resizing {}",
                    partition.as_str()
                )
            }
        },
    };
    let variant = plan.linux_variant().label();
    let boot_mode = plan.boot_mode().to_string();
    let verification = if target {
        "Named block devices exist. Install readiness still depends on the source and target."
    } else {
        "Use --target on the install host to check that the named block devices exist."
    };
    crate::ui::emit(
        crate::ui::Level::Success,
        "arch.config.valid",
        &format!(
            "Configuration valid for {variant} in {boot_mode} mode.\nTarget disk {disk}\n{operation}.\n{verification}"
        ),
        Some(serde_json::json!({
            "disk": disk,
            "boot_mode": boot_mode,
            "linux_variant": plan.linux_variant().id(),
            "operation": operation,
            "target_devices_checked": target,
        })),
    );
    Ok(())
}

fn require_block_device(path: &str) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("Cannot access target device {path}"))?;
    ensure!(
        metadata.file_type().is_block_device(),
        "{path} is not a block device"
    );
    Ok(())
}
