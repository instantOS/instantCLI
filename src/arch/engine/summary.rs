use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::context::InstallContext;
use super::types::{BootMode, QuestionId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartitioningKind {
    Automatic,
    DualBoot,
    Manual,
    Unknown,
}

impl std::fmt::Display for PartitioningKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitioningKind::Automatic => write!(f, "automatic"),
            PartitioningKind::DualBoot => write!(f, "dual-boot"),
            PartitioningKind::Manual => write!(f, "manual"),
            PartitioningKind::Unknown => write!(f, "unknown"),
        }
    }
}

pub(crate) struct InstallSummary {
    pub(crate) text: String,
    pub(crate) partitioning_kind: PartitioningKind,
}

fn partitioning_kind_from(method: &str) -> PartitioningKind {
    if method.contains("Dual Boot") {
        PartitioningKind::DualBoot
    } else if method.contains("Manual") {
        PartitioningKind::Manual
    } else if method.contains("Automatic") {
        PartitioningKind::Automatic
    } else {
        PartitioningKind::Unknown
    }
}

fn format_disk_label(context: &InstallContext) -> String {
    let Some(disk) = context.get_answer(&QuestionId::Disk) else {
        return "<not set>".to_string();
    };

    let disk = disk.to_string();

    if let Some(entries) = context.get::<crate::arch::disks::DisksKey>()
        && let Some(entry) = entries.iter().find(|entry| entry.path == disk)
    {
        return format!("{} ({})", disk, entry.size);
    }

    if let Some(entries) = context.get::<crate::arch::dualboot::DisksKey>()
        && let Some(info) = entries.iter().find(|info| info.device == disk)
    {
        return format!("{} ({})", disk, info.size_human());
    }

    disk
}

fn answer_or(context: &InstallContext, id: QuestionId, fallback: &str) -> String {
    context
        .get_answer(&id)
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn format_dualboot_size(context: &InstallContext) -> Option<String> {
    context.get_answer(&QuestionId::DualBootSize).map(|value| {
        value
            .parse::<u64>()
            .map(crate::arch::dualboot::format_size)
            .unwrap_or_else(|_| value.clone())
    })
}

fn format_dualboot_resize_method(context: &InstallContext, uses_free_space: bool) -> String {
    if uses_free_space {
        return "Not required".to_string();
    }

    match context
        .get_answer(&QuestionId::DualBootInstructions)
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
    let hostname = answer_or(context, QuestionId::Hostname, "<not set>");
    let username = answer_or(context, QuestionId::Username, "<not set>");

    let timezone = answer_or(context, QuestionId::Timezone, "<not set>");
    let locale = answer_or(context, QuestionId::Locale, "<not set>");
    let keymap = answer_or(context, QuestionId::Keymap, "<not set>");

    let partitioning_method = answer_or(context, QuestionId::PartitioningMethod, "<not set>");
    let partitioning_kind = partitioning_kind_from(&partitioning_method);

    let disk = format_disk_label(context);

    let mirror_region = context
        .get_answer(&QuestionId::MirrorRegion)
        .cloned()
        .unwrap_or_else(|| "Fallback (auto)".to_string());

    let minimal_mode = context.get_answer_bool(QuestionId::MinimalMode);
    let profile = if minimal_mode {
        "Minimal (vanilla Arch)".to_string()
    } else {
        "instantOS (full)".to_string()
    };

    let kernel = context
        .get_answer(&QuestionId::Kernel)
        .cloned()
        .unwrap_or_else(|| "linux (default)".to_string());

    let use_plymouth = context.get_answer_bool(QuestionId::UsePlymouth);
    let plymouth_label = if minimal_mode {
        "Disabled (minimal mode)".to_string()
    } else if use_plymouth {
        "Enabled".to_string()
    } else {
        "Disabled".to_string()
    };

    let autologin_label = if minimal_mode {
        "Disabled (minimal mode)".to_string()
    } else if context.get_answer_bool(QuestionId::Autologin) {
        "Enabled".to_string()
    } else {
        "Disabled".to_string()
    };

    let log_upload_label = if context.get_answer_bool(QuestionId::LogUpload) {
        "Upload to snips.sh".to_string()
    } else {
        "Do not upload".to_string()
    };

    let encryption_label = match partitioning_kind {
        PartitioningKind::Automatic => {
            if context.get_answer_bool(QuestionId::UseEncryption) {
                "Enabled (LUKS)".to_string()
            } else {
                "Disabled".to_string()
            }
        }
        PartitioningKind::DualBoot => "Not supported for dual boot".to_string(),
        PartitioningKind::Manual => "Not supported for manual partitioning".to_string(),
        PartitioningKind::Unknown => {
            if context.get_answer_bool(QuestionId::UseEncryption) {
                "Enabled (LUKS)".to_string()
            } else {
                "Disabled".to_string()
            }
        }
    };

    let user_password_status = if context.get_answer(&QuestionId::Password).is_some() {
        "Set"
    } else {
        "Not set"
    };

    let encryption_password_status = if context
        .get_answer(&QuestionId::EncryptionPassword)
        .is_some()
    {
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
        .field_indented("Partitioning", &partitioning_method);

    match partitioning_kind {
        PartitioningKind::Automatic => {
            let layout = format_automatic_layout(
                context,
                context.get_answer_bool(QuestionId::UseEncryption),
            );
            builder = builder
                .field_indented("Layout", &layout)
                .field_indented("Swap", "Auto (RAM-based)");
        }
        PartitioningKind::DualBoot => {
            let resize_target = match context.get_answer(&QuestionId::DualBootPartition) {
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
        PartitioningKind::Manual => {
            let root_partition = answer_or(context, QuestionId::RootPartition, "<not set>");
            let boot_partition = answer_or(context, QuestionId::BootPartition, "<not set>");
            let swap_partition = context
                .get_answer(&QuestionId::SwapPartition)
                .cloned()
                .unwrap_or_else(|| "none".to_string());
            let home_partition = context
                .get_answer(&QuestionId::HomePartition)
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
        PartitioningKind::Unknown => {}
    }

    builder = builder
        .blank()
        .line(colors::TEAL, Some(NerdFont::Lock), "Security")
        .field_indented("Disk encryption", &encryption_label)
        .field_indented("User password", user_password_status);

    if partitioning_kind == PartitioningKind::Automatic
        && context.get_answer_bool(QuestionId::UseEncryption)
    {
        builder = builder.field_indented("LUKS passphrase", encryption_password_status);
    }

    builder = builder
        .blank()
        .line(colors::TEAL, Some(NerdFont::Sliders), "System Options")
        .field_indented("Kernel", &kernel)
        .field_indented("Profile", &profile)
        .field_indented("Plymouth", &plymouth_label)
        .field_indented("Autologin", &autologin_label)
        .field_indented("Log upload", &log_upload_label)
        .field_indented("Mirror region", &mirror_region);

    let summary = builder.build_string();
    let summary = summary.trim_start_matches('\n').to_string();

    InstallSummary {
        text: summary,
        partitioning_kind,
    }
}
