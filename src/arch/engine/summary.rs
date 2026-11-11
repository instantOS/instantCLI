use crate::arch::config::{
    BTRFS_HOME_SUBVOLUME, BTRFS_ROOT_SUBVOLUME, BtrfsCompression, DesktopEnvironment,
    DisplayManager, RootFilesystem,
};
use crate::common::format::format_size;
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewWriter;

use super::context::InstallContext;
use super::install_plan::{DualBootResizeMethod, FREE_SPACE_ANSWER};
use super::types::{BootMode, PartitioningMethod, StepId};

pub(crate) struct InstallSummary {
    pub(crate) text: String,
    pub(crate) partitioning_method: Option<PartitioningMethod>,
}

const NOT_SET: &str = "<not set>";

/// Human-readable values shown on the installer review screen.
///
/// Every answer-to-label rule lives here, so the render functions below only
/// place text and never branch on wizard state. Keeping the rules in one
/// struct also makes them testable without parsing styled preview output.
struct SummaryFacts {
    identity: IdentityFacts,
    locale: LocaleFacts,
    storage: StorageFacts,
    security: SecurityFacts,
    system: SystemFacts,
}

struct IdentityFacts {
    hostname: String,
    username: String,
}

struct LocaleFacts {
    timezone: String,
    locale: String,
    keymap: String,
    console_font: String,
}

struct StorageFacts {
    disk: String,
    partitioning: Option<PartitioningMethod>,
    root_filesystem: String,
    details: Option<StorageDetails>,
}

/// Partitioning-specific rows of the storage section.
enum StorageDetails {
    Automatic {
        layout: String,
    },
    DualBoot {
        resize_target: String,
        linux_size: String,
        resize_method: String,
    },
    Manual {
        root: String,
        boot: String,
        swap: String,
        home: String,
    },
}

struct SecurityFacts {
    encryption: String,
    user_password: &'static str,
    luks_passphrase: Option<&'static str>,
}

struct SystemFacts {
    kernel: String,
    profile: &'static str,
    desktop: String,
    display_manager: String,
    plymouth: &'static str,
    xorg: &'static str,
    autologin: &'static str,
    log_upload: &'static str,
    mirror_region: String,
}

impl SummaryFacts {
    fn from_context(context: &InstallContext) -> Self {
        let minimal_mode = context.get_answer_bool(StepId::MinimalMode);
        let desktop = DesktopEnvironment::selected_or_default(context);
        let use_encryption = context.get_answer_bool(StepId::UseEncryption);
        let partitioning = context.partitioning_method();
        let requires_display_manager = desktop.requires_display_manager();

        Self {
            identity: IdentityFacts {
                hostname: answer_or(context, StepId::Hostname, NOT_SET),
                username: answer_or(context, StepId::Username, NOT_SET),
            },
            locale: LocaleFacts {
                timezone: answer_or(context, StepId::Timezone, NOT_SET),
                locale: answer_or(context, StepId::Locale, NOT_SET),
                keymap: answer_or(context, StepId::Keymap, NOT_SET),
                console_font: crate::common::console_font::answer_label(&answer_or(
                    context,
                    StepId::ConsoleFont,
                    "default8x16",
                )),
            },
            storage: StorageFacts {
                disk: format_disk_label(context),
                partitioning,
                root_filesystem: format_filesystem_label(context),
                details: format_storage_details(context, partitioning, use_encryption),
            },
            security: SecurityFacts {
                encryption: format_encryption_label(partitioning, use_encryption),
                user_password: password_status(context, StepId::Password),
                luks_passphrase: (partitioning == Some(PartitioningMethod::Automatic)
                    && use_encryption)
                    .then(|| password_status(context, StepId::EncryptionPassword)),
            },
            system: SystemFacts {
                kernel: answer_or(context, StepId::Kernel, "linux (default)"),
                profile: if minimal_mode {
                    "Minimal (vanilla Arch)"
                } else {
                    "instantOS (full)"
                },
                desktop: if minimal_mode {
                    "Skipped (minimal mode)".to_string()
                } else {
                    desktop.label().to_string()
                },
                display_manager: format_display_manager_label(context, desktop, minimal_mode),
                plymouth: optional_feature_label(
                    context.get_answer_bool(StepId::UsePlymouth),
                    minimal_mode,
                    true,
                ),
                xorg: optional_feature_label(
                    context.get_answer_bool(StepId::UseXorg),
                    minimal_mode,
                    requires_display_manager,
                ),
                autologin: optional_feature_label(
                    context.get_answer_bool(StepId::Autologin),
                    minimal_mode,
                    requires_display_manager,
                ),
                log_upload: if context.get_answer_bool(StepId::LogUpload) {
                    "Upload to snips.sh"
                } else {
                    "Do not upload"
                },
                mirror_region: context
                    .get_answer(&StepId::MirrorRegion)
                    .cloned()
                    .unwrap_or_else(|| "Fallback (auto)".to_string()),
            },
        }
    }
}

fn answer_or(context: &InstallContext, id: StepId, fallback: &str) -> String {
    context
        .get_answer(&id)
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn password_status(context: &InstallContext, id: StepId) -> &'static str {
    if context.get_answer(&id).is_some() {
        "Set"
    } else {
        "Not set"
    }
}

fn format_disk_label(context: &InstallContext) -> String {
    let Some(disk) = context.get_answer(&StepId::Disk) else {
        return NOT_SET.to_string();
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

fn format_filesystem_label(context: &InstallContext) -> String {
    let root_filesystem = RootFilesystem::selected_or_default(context);
    if !root_filesystem.is_btrfs() {
        return "ext4".to_string();
    }

    let compression = BtrfsCompression::selected_or_default(context);
    let subvolumes = if context.get_answer(&StepId::HomePartition).is_some() {
        BTRFS_ROOT_SUBVOLUME.to_string()
    } else {
        format!("{BTRFS_ROOT_SUBVOLUME}, {BTRFS_HOME_SUBVOLUME}")
    };
    format!("btrfs ({subvolumes}; {})", compression.label())
}

fn format_encryption_label(
    partitioning: Option<PartitioningMethod>,
    use_encryption: bool,
) -> String {
    match partitioning {
        Some(PartitioningMethod::DualBoot) => "Not supported for dual boot".to_string(),
        Some(PartitioningMethod::Manual) => "Not supported for manual partitioning".to_string(),
        Some(PartitioningMethod::Automatic) | None => {
            if use_encryption {
                "Enabled (LUKS)".to_string()
            } else {
                "Disabled".to_string()
            }
        }
    }
}

/// Label for an optional on/off feature.
///
/// Minimal mode turns these off regardless of the stored answer. Features that
/// do not apply to the selected desktop are `Not required` rather than a
/// misleading `Disabled`.
fn optional_feature_label(enabled: bool, minimal_mode: bool, applicable: bool) -> &'static str {
    match (minimal_mode, applicable, enabled) {
        (true, _, _) => "Disabled (minimal mode)",
        (false, false, _) => "Not required",
        (false, true, true) => "Enabled",
        (false, true, false) => "Disabled",
    }
}

/// Label for a component that minimal mode suppresses entirely.
fn skipped_in_minimal_mode(minimal_mode: bool) -> Option<&'static str> {
    minimal_mode.then_some("Skipped (minimal mode)")
}

/// Display manager for the selected desktop, or `Not required` when the
/// desktop does not use one.
fn display_manager_label(context: &InstallContext, desktop: DesktopEnvironment) -> &'static str {
    if desktop.requires_display_manager() {
        DisplayManager::selected_or_default(context).label()
    } else {
        "Not required"
    }
}

fn format_display_manager_label(
    context: &InstallContext,
    desktop: DesktopEnvironment,
    minimal_mode: bool,
) -> String {
    skipped_in_minimal_mode(minimal_mode)
        .unwrap_or_else(|| display_manager_label(context, desktop))
        .to_string()
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

fn format_storage_details(
    context: &InstallContext,
    partitioning: Option<PartitioningMethod>,
    use_encryption: bool,
) -> Option<StorageDetails> {
    match partitioning? {
        PartitioningMethod::Automatic => Some(StorageDetails::Automatic {
            layout: format_automatic_layout(context, use_encryption),
        }),
        PartitioningMethod::DualBoot => {
            // `uses_free_space` is derived from the stored answer, before it is
            // turned into a label; the label is display text, not a dispatch key.
            let (resize_target, uses_free_space) =
                match context.get_answer(&StepId::DualBootPartition) {
                    Some(value) if value == FREE_SPACE_ANSWER => {
                        ("Use existing free space".to_string(), true)
                    }
                    Some(value) => (value.clone(), false),
                    None => (NOT_SET.to_string(), false),
                };

            Some(StorageDetails::DualBoot {
                resize_target,
                linux_size: format_dualboot_size(context).unwrap_or_else(|| NOT_SET.to_string()),
                resize_method: format_dualboot_resize_method(context, uses_free_space),
            })
        }
        PartitioningMethod::Manual => Some(StorageDetails::Manual {
            root: answer_or(context, StepId::RootPartition, NOT_SET),
            boot: answer_or(context, StepId::BootPartition, NOT_SET),
            swap: context
                .get_answer(&StepId::SwapPartition)
                .cloned()
                .unwrap_or_else(|| "none".to_string()),
            home: context
                .get_answer(&StepId::HomePartition)
                .cloned()
                .unwrap_or_else(|| "none".to_string()),
        }),
    }
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
        .and_then(|value| DualBootResizeMethod::parse(value).ok())
    {
        Some(DualBootResizeMethod::Auto) => "Installer resize".to_string(),
        Some(DualBootResizeMethod::Confirmed) | None => "Manual resize".to_string(),
    }
}

fn render_identity(writer: &mut PreviewWriter, facts: &IdentityFacts) {
    writer
        .line(colors::TEAL, Some(NerdFont::User), "Identity")
        .field_indented("Hostname", &facts.hostname)
        .field_indented("Username", &facts.username);
}

fn render_locale(writer: &mut PreviewWriter, facts: &LocaleFacts) {
    writer
        .blank()
        .line(colors::TEAL, Some(NerdFont::Language), "Locale & Input")
        .field_indented("Timezone", &facts.timezone)
        .field_indented("Locale", &facts.locale)
        .field_indented("Keymap", &facts.keymap)
        .field_indented("TTY font", &facts.console_font);
}

fn render_storage(writer: &mut PreviewWriter, facts: &StorageFacts) {
    let partitioning = facts
        .partitioning
        .map(|kind| kind.to_string())
        .unwrap_or_else(|| NOT_SET.to_string());

    writer
        .blank()
        .line(colors::TEAL, Some(NerdFont::HardDrive), "Storage Plan")
        .field_indented("Disk", &facts.disk)
        .field_indented("Partitioning", &partitioning)
        .field_indented("Root filesystem", &facts.root_filesystem);

    match &facts.details {
        Some(StorageDetails::Automatic { layout }) => {
            writer
                .field_indented("Layout", layout)
                .field_indented("Swap", "Auto (RAM-based)");
        }
        Some(StorageDetails::DualBoot {
            resize_target,
            linux_size,
            resize_method,
        }) => {
            writer
                .blank()
                .line(colors::TEAL, Some(NerdFont::Partition), "Dual Boot")
                .field_indented("Resize target", resize_target)
                .field_indented("Linux size", linux_size)
                .field_indented("Resize method", resize_method)
                .field_indented("Swap", "Auto (RAM-based)");
        }
        Some(StorageDetails::Manual {
            root,
            boot,
            swap,
            home,
        }) => {
            writer
                .blank()
                .line(colors::TEAL, Some(NerdFont::Partition), "Partitions")
                .field_indented("Root", root)
                .field_indented("Boot/EFI", boot)
                .field_indented("Swap", swap)
                .field_indented("Home", home);
        }
        None => {}
    }
}

fn render_security(writer: &mut PreviewWriter, facts: &SecurityFacts) {
    writer
        .blank()
        .line(colors::TEAL, Some(NerdFont::Lock), "Security")
        .field_indented("Disk encryption", &facts.encryption)
        .field_indented("User password", facts.user_password);

    if let Some(passphrase) = facts.luks_passphrase {
        writer.field_indented("LUKS passphrase", passphrase);
    }
}

fn render_system(writer: &mut PreviewWriter, facts: &SystemFacts) {
    writer
        .blank()
        .line(colors::TEAL, Some(NerdFont::Sliders), "System Options")
        .field_indented("Kernel", &facts.kernel)
        .field_indented("Desktop", &facts.desktop)
        .field_indented("Display manager", &facts.display_manager)
        .field_indented("Profile", facts.profile)
        .field_indented("Plymouth", facts.plymouth)
        .field_indented("Xorg server", facts.xorg)
        .field_indented("Autologin", facts.autologin)
        .field_indented("Log upload", facts.log_upload)
        .field_indented("Mirror region", &facts.mirror_region);
}

pub(crate) fn build_install_summary(context: &InstallContext) -> InstallSummary {
    let facts = SummaryFacts::from_context(context);

    let mut writer = PreviewWriter::collect();
    render_identity(&mut writer, &facts.identity);
    render_locale(&mut writer, &facts.locale);
    render_storage(&mut writer, &facts.storage);
    render_security(&mut writer, &facts.security);
    render_system(&mut writer, &facts.system);

    InstallSummary {
        text: writer.build_string().trim_start_matches('\n').to_string(),
        partitioning_method: facts.storage.partitioning,
    }
}

/// Compact summary for the setup flow's final review screen.
///
/// Unlike the install summary, there is no partitioning or package plan to
/// describe: `ins arch setup` only converts an existing system to instantOS.
pub(crate) fn build_setup_summary(context: &InstallContext) -> String {
    let username = answer_or(context, StepId::Username, NOT_SET);

    let desktop = DesktopEnvironment::selected_or_default(context);
    let requires_display_manager = desktop.requires_display_manager();

    let mut writer = PreviewWriter::collect();
    writer
        .line(colors::TEAL, Some(NerdFont::User), "Setup")
        .field_indented("User", &username)
        .field_indented("Desktop", desktop.label())
        .field_indented("Display manager", display_manager_label(context, desktop))
        .field_indented(
            "Autologin",
            optional_feature_label(
                context.get_answer_bool(StepId::Autologin),
                false,
                requires_display_manager,
            ),
        )
        .field_indented(
            "Xorg server",
            optional_feature_label(
                context.get_answer_bool(StepId::UseXorg),
                false,
                requires_display_manager,
            ),
        );

    writer.build_string().trim_start_matches('\n').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_with(answers: &[(StepId, &str)]) -> InstallContext {
        let mut context = InstallContext::new();
        for (id, answer) in answers {
            context.set_answer(*id, (*answer).to_string());
        }
        context
    }

    fn dualboot_details(facts: &SummaryFacts) -> (&str, &str, &str) {
        match &facts.storage.details {
            Some(StorageDetails::DualBoot {
                resize_target,
                linux_size,
                resize_method,
            }) => (resize_target, linux_size, resize_method),
            _ => panic!("expected dual-boot storage details"),
        }
    }

    #[test]
    fn minimal_mode_overrides_optional_features() {
        let context = context_with(&[
            (StepId::MinimalMode, "yes"),
            (StepId::DesktopEnvironment, "sway"),
            (StepId::UsePlymouth, "yes"),
            (StepId::UseXorg, "yes"),
            (StepId::Autologin, "yes"),
        ]);

        let facts = SummaryFacts::from_context(&context);

        assert_eq!(facts.system.profile, "Minimal (vanilla Arch)");
        assert_eq!(facts.system.desktop, "Skipped (minimal mode)");
        assert_eq!(facts.system.display_manager, "Skipped (minimal mode)");
        assert_eq!(facts.system.plymouth, "Disabled (minimal mode)");
        assert_eq!(facts.system.xorg, "Disabled (minimal mode)");
        assert_eq!(facts.system.autologin, "Disabled (minimal mode)");
    }

    #[test]
    fn tty_desktop_marks_display_manager_features_not_required() {
        let context = context_with(&[
            (StepId::DesktopEnvironment, "none/tty"),
            (StepId::Autologin, "yes"),
            (StepId::UseXorg, "yes"),
        ]);

        let facts = SummaryFacts::from_context(&context);

        assert_eq!(facts.system.desktop, "None / TTY");
        assert_eq!(facts.system.display_manager, "Not required");
        assert_eq!(facts.system.autologin, "Not required");
        assert_eq!(facts.system.xorg, "Not required");
    }

    #[test]
    fn dualboot_free_space_target_does_not_require_a_resize() {
        let context = context_with(&[
            (StepId::PartitioningMethod, "dual_boot"),
            (StepId::DualBootPartition, FREE_SPACE_ANSWER),
        ]);

        let facts = SummaryFacts::from_context(&context);
        let (target, _, method) = dualboot_details(&facts);

        assert_eq!(target, "Use existing free space");
        assert_eq!(method, "Not required");
    }

    #[test]
    fn dualboot_automatic_resize_reports_installer_resize() {
        let context = context_with(&[
            (StepId::PartitioningMethod, "dual_boot"),
            (StepId::DualBootPartition, "/dev/sda3"),
            (StepId::DualBootInstructions, "auto"),
            (StepId::DualBootSize, "1073741824"),
        ]);

        let facts = SummaryFacts::from_context(&context);
        let (target, size, method) = dualboot_details(&facts);

        assert_eq!(target, "/dev/sda3");
        assert_eq!(size, "1.0 GB");
        assert_eq!(method, "Installer resize");
    }

    #[test]
    fn btrfs_home_subvolume_is_listed_only_without_a_home_partition() {
        let mut context = context_with(&[(StepId::RootFilesystem, "btrfs")]);

        let without_home = SummaryFacts::from_context(&context);
        assert_eq!(
            without_home.storage.root_filesystem,
            "btrfs (@, @home; zstd (recommended))"
        );

        context.set_answer(StepId::HomePartition, "/dev/sda4".to_string());
        let with_home = SummaryFacts::from_context(&context);
        assert_eq!(
            with_home.storage.root_filesystem,
            "btrfs (@; zstd (recommended))"
        );
    }

    #[test]
    fn unanswered_answers_use_placeholders() {
        let facts = SummaryFacts::from_context(&InstallContext::new());

        assert_eq!(facts.identity.hostname, NOT_SET);
        assert_eq!(facts.storage.disk, NOT_SET);
        assert_eq!(facts.storage.partitioning, None);
        assert!(facts.storage.details.is_none());
        assert_eq!(facts.security.encryption, "Disabled");
        assert_eq!(facts.security.user_password, "Not set");
        assert_eq!(facts.security.luks_passphrase, None);
    }

    #[test]
    fn automatic_encryption_shows_luks_passphrase_status() {
        let context = context_with(&[
            (StepId::PartitioningMethod, "automatic"),
            (StepId::UseEncryption, "yes"),
            (StepId::EncryptionPassword, "secret"),
        ]);

        let facts = SummaryFacts::from_context(&context);

        assert_eq!(facts.security.encryption, "Enabled (LUKS)");
        assert_eq!(facts.security.luks_passphrase, Some("Set"));
    }

    #[test]
    fn uefi_automatic_layout_mentions_efi() {
        let mut context = context_with(&[(StepId::PartitioningMethod, "automatic")]);
        context.system_info.boot_mode = BootMode::UEFI64;

        let facts = SummaryFacts::from_context(&context);

        match &facts.storage.details {
            Some(StorageDetails::Automatic { layout }) => {
                assert_eq!(layout, "EFI (1 GiB) + Swap (auto) + Root");
            }
            _ => panic!("expected automatic storage details"),
        }
    }

    #[test]
    fn install_summary_returns_partitioning_method_without_leading_blank_line() {
        let context = context_with(&[(StepId::PartitioningMethod, "automatic")]);

        let summary = build_install_summary(&context);
        let text = crate::ui::text::strip_ansi(&summary.text);

        assert_eq!(
            summary.partitioning_method,
            Some(PartitioningMethod::Automatic)
        );
        assert!(!summary.text.starts_with('\n'));
        assert!(text.contains("Storage Plan"));
        assert!(text.contains("Disk encryption: Disabled"));
    }

    #[test]
    fn setup_summary_marks_display_manager_features_not_required_for_tty() {
        let context = context_with(&[(StepId::DesktopEnvironment, "none/tty")]);

        let text = crate::ui::text::strip_ansi(&build_setup_summary(&context));

        assert!(text.contains("Display manager: Not required"));
        assert!(text.contains("Autologin: Not required"));
        assert!(text.contains("Xorg server: Not required"));
    }
}
