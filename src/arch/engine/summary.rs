use crate::common::format::format_size;
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::context::InstallContext;
use super::types::{BootMode, PartitioningMethod, StepId};

pub(crate) struct InstallSummary {
    pub(crate) text: String,
    pub(crate) partitioning_method: Option<PartitioningMethod>,
}

fn format_disk_label(context: &InstallContext) -> String {
    let Some(disk) = context.get_answer(&StepId::Disk) else {
        return "<not set>".to_string();
    };

    let disk = disk.to_string();

    if let Some(entries) = context.get::<crate::arch::disks::DisksKey>()
        && let Some(entry) = entries.iter().find(|entry| entry.path == disk)
    {
        return format!("{} ({})", disk, entry.size);
    }

    if let Some(entries) = context.get::<crate::arch::dualboot::DualBootDisksKey>()
        && let Some(info) = entries.iter().find(|info| info.device == disk)
    {
        return format!("{} ({})", disk, info.size_human());
    }

    disk
}

fn answer_or(context: &InstallContext, id: StepId, fallback: &str) -> String {
    context
        .get_answer(&id)
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn format_dualboot_size(context: &InstallContext) -> Option<String> {
    context.get_answer(&StepId::DualBootSize).map(|value| {
        value
            .parse::<u64>()
            .map(format_size)
            .unwrap_or_else(|_| value.clone())
    })
}

fn format_dualboot_resize_method(context: &InstallContext, uses_free_space: bool) -> String {
    if uses_free_space {
        return "Not required".to_string();
    }

    match context
        .get_answer(&StepId::DualBootInstructions)
        .map(|value| value.as_str())
    {
        Some("auto") => "Installer resize".to_string(),
        Some("confirmed") => "Manual resize".to_string(),
        _ => "Manual resize".to_string(),
    }
}

fn format_automatic_layout(context: &InstallContext, use_encryption: bool) -> String {
    match (context.system_info.boot_mode.clone(), use_encryption) {
        (BootMode::UEFI64 | BootMode::UEFI32, true) => {
            "EFI (1 GiB) + LUKS (LVM swap + root)".to_string()
        }
        (BootMode::UEFI64 | BootMode::UEFI32, false) => {
            "EFI (1 GiB) + Swap (auto) + Root".to_string()
        }
        (BootMode::BIOS, true) => "Boot (1 GiB) + LUKS (LVM swap + root)".to_string(),
        (BootMode::BIOS, false) => "Swap (auto) + Root".to_string(),
    }
}

pub(crate) fn build_install_summary(context: &InstallContext) -> InstallSummary {
    let hostname = answer_or(context, StepId::Hostname, "<not set>");
    let username = answer_or(context, StepId::Username, "<not set>");

    let timezone = answer_or(context, StepId::Timezone, "<not set>");
    let locale = answer_or(context, StepId::Locale, "<not set>");
    let keymap = answer_or(context, StepId::Keymap, "<not set>");

    let partitioning_method = context.partitioning_method();

    let disk = format_disk_label(context);

    let mirror_region = context
        .get_answer(&StepId::MirrorRegion)
        .cloned()
        .unwrap_or_else(|| "Fallback (auto)".to_string());

    let minimal_mode = context.get_answer_bool(StepId::MinimalMode);
    let profile = if minimal_mode {
        "Minimal (vanilla Arch)".to_string()
    } else {
        "instantOS (full)".to_string()
    };

    let kernel = context
        .get_answer(&StepId::Kernel)
        .cloned()
        .unwrap_or_else(|| "linux (default)".to_string());

    let desktop_label = if minimal_mode {
        "Skipped (minimal mode)".to_string()
    } else {
        crate::arch::config::DesktopEnvironment::selected_or_default(context)
            .label()
            .to_string()
    };

    let root_filesystem = crate::arch::config::RootFilesystem::selected_or_default(context);
    let filesystem_label = if root_filesystem.is_btrfs() {
        let compression = crate::arch::config::BtrfsCompression::selected_or_default(context);
        let subvolumes = if context.get_answer(&StepId::HomePartition).is_some() {
            "@"
        } else {
            "@, @home"
        };
        format!("btrfs ({subvolumes}; {})", compression.label())
    } else {
        "ext4".to_string()
    };

    let use_plymouth = context.get_answer_bool(StepId::UsePlymouth);
    let plymouth_label = if minimal_mode {
        "Disabled (minimal mode)".to_string()
    } else if use_plymouth {
        "Enabled".to_string()
    } else {
        "Disabled".to_string()
    };
    let use_xorg = context.get_answer_bool(StepId::UseXorg);
    let xorg_label = if minimal_mode {
        "Disabled (minimal mode)".to_string()
    } else if use_xorg {
        "Enabled".to_string()
    } else {
        "Disabled".to_string()
    };
    let autologin_label = if minimal_mode {
        "Disabled (minimal mode)".to_string()
    } else if context.get_answer_bool(StepId::Autologin) {
        "Enabled".to_string()
    } else {
        "Disabled".to_string()
    };

    let dm_label = if minimal_mode {
        "Skipped (minimal mode)".to_string()
    } else if crate::arch::config::DesktopEnvironment::selected_or_default(context)
        .requires_display_manager()
    {
        crate::arch::config::DisplayManager::selected_or_default(context)
            .label()
            .to_string()
    } else {
        "Not required".to_string()
    };

    let log_upload_label = if context.get_answer_bool(StepId::LogUpload) {
        "Upload to snips.sh".to_string()
    } else {
        "Do not upload".to_string()
    };

    let encryption_label = match partitioning_method {
        Some(PartitioningMethod::Automatic) => {
            if context.get_answer_bool(StepId::UseEncryption) {
                "Enabled (LUKS)".to_string()
            } else {
                "Disabled".to_string()
            }
        }
        Some(PartitioningMethod::DualBoot) => "Not supported for dual boot".to_string(),
        Some(PartitioningMethod::Manual) => "Not supported for manual partitioning".to_string(),
        None => {
            if context.get_answer_bool(StepId::UseEncryption) {
                "Enabled (LUKS)".to_string()
            } else {
                "Disabled".to_string()
            }
        }
    };

    let user_password_status = if context.get_answer(&StepId::Password).is_some() {
        "Set"
    } else {
        "Not set"
    };

    let encryption_password_status = if context.get_answer(&StepId::EncryptionPassword).is_some() {
        "Set"
    } else {
        "Not set"
    };

    let mut builder = PreviewBuilder::new()
        .line(colors::TEAL, Some(NerdFont::User), "Identity")
        .field_indented("Hostname", &hostname)
        .field_indented("Username", &username)
        .blank()
        .line(colors::TEAL, Some(NerdFont::Language), "Locale & Input")
        .field_indented("Timezone", &timezone)
        .field_indented("Locale", &locale)
        .field_indented("Keymap", &keymap)
        .blank()
        .line(colors::TEAL, Some(NerdFont::HardDrive), "Storage Plan")
        .field_indented("Disk", &disk)
        .field_indented(
            "Partitioning",
            &partitioning_method
                .map(|kind| kind.to_string())
                .unwrap_or_else(|| "<not set>".to_string()),
        )
        .field_indented("Root filesystem", &filesystem_label);

    match partitioning_method {
        Some(PartitioningMethod::Automatic) => {
            let layout =
                format_automatic_layout(context, context.get_answer_bool(StepId::UseEncryption));
            builder = builder
                .field_indented("Layout", &layout)
                .field_indented("Swap", "Auto (RAM-based)");
        }
        Some(PartitioningMethod::DualBoot) => {
            let resize_target = match context.get_answer(&StepId::DualBootPartition) {
                Some(value) if value == "__free_space__" => "Use existing free space".to_string(),
                Some(value) => value.clone(),
                None => "<not set>".to_string(),
            };
            let uses_free_space = resize_target == "Use existing free space";
            let linux_size =
                format_dualboot_size(context).unwrap_or_else(|| "<not set>".to_string());
            let resize_method = format_dualboot_resize_method(context, uses_free_space);

            builder = builder
                .blank()
                .line(colors::TEAL, Some(NerdFont::Partition), "Dual Boot")
                .field_indented("Resize target", &resize_target)
                .field_indented("Linux size", &linux_size)
                .field_indented("Resize method", &resize_method)
                .field_indented("Swap", "Auto (RAM-based)");
        }
        Some(PartitioningMethod::Manual) => {
            let root_partition = answer_or(context, StepId::RootPartition, "<not set>");
            let boot_partition = answer_or(context, StepId::BootPartition, "<not set>");
            let swap_partition = context
                .get_answer(&StepId::SwapPartition)
                .cloned()
                .unwrap_or_else(|| "none".to_string());
            let home_partition = context
                .get_answer(&StepId::HomePartition)
                .cloned()
                .unwrap_or_else(|| "none".to_string());

            builder = builder
                .blank()
                .line(colors::TEAL, Some(NerdFont::Partition), "Partitions")
                .field_indented("Root", &root_partition)
                .field_indented("Boot/EFI", &boot_partition)
                .field_indented("Swap", &swap_partition)
                .field_indented("Home", &home_partition);
        }
        None => {}
    }

    builder = builder
        .blank()
        .line(colors::TEAL, Some(NerdFont::Lock), "Security")
        .field_indented("Disk encryption", &encryption_label)
        .field_indented("User password", user_password_status);

    if partitioning_method == Some(PartitioningMethod::Automatic)
        && context.get_answer_bool(StepId::UseEncryption)
    {
        builder = builder.field_indented("LUKS passphrase", encryption_password_status);
    }

    builder = builder
        .blank()
        .line(colors::TEAL, Some(NerdFont::Sliders), "System Options")
        .field_indented("Kernel", &kernel)
        .field_indented("Desktop", &desktop_label)
        .field_indented("Display manager", &dm_label)
        .field_indented("Profile", &profile)
        .field_indented("Plymouth", &plymouth_label)
        .field_indented("Xorg server", &xorg_label)
        .field_indented("Autologin", &autologin_label)
        .field_indented("Log upload", &log_upload_label)
        .field_indented("Mirror region", &mirror_region);

    let summary = builder.build_string();
    let summary = summary.trim_start_matches('\n').to_string();

    InstallSummary {
        text: summary,
        partitioning_method,
    }
}

/// Compact summary for the setup flow's final review screen.
///
/// Unlike the install summary, there is no partitioning or package plan to
/// describe: `ins arch setup` only converts an existing system to instantOS.
pub(crate) fn build_setup_summary(context: &InstallContext) -> String {
    let username = answer_or(context, StepId::Username, "<not set>");

    let desktop = crate::arch::config::DesktopEnvironment::selected_or_default(context);
    let dm_label = if desktop.requires_display_manager() {
        crate::arch::config::DisplayManager::selected_or_default(context)
            .label()
            .to_string()
    } else {
        "Not required".to_string()
    };
    let autologin_label = if desktop.requires_display_manager() {
        if context.get_answer_bool(StepId::Autologin) {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        }
    } else {
        "Not required".to_string()
    };
    let xorg_label = if desktop.requires_display_manager() {
        if context.get_answer_bool(StepId::UseXorg) {
            "Enabled".to_string()
        } else {
            "Disabled".to_string()
        }
    } else {
        "Not required".to_string()
    };

    let summary = PreviewBuilder::new()
        .line(colors::TEAL, Some(NerdFont::User), "Setup")
        .field_indented("User", &username)
        .field_indented("Desktop", desktop.label())
        .field_indented("Display manager", &dm_label)
        .field_indented("Autologin", &autologin_label)
        .field_indented("Xorg server", &xorg_label)
        .build_string();

    summary.trim_start_matches('\n').to_string()
}
