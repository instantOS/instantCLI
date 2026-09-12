use std::fmt;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};

use super::{BootMode, InstallContext, Kernel, PartitioningMethod, StepId, SystemInfo};
use crate::arch::config::{BtrfsCompression, DesktopEnvironment, DisplayManager, RootFilesystem};

/// Accessor for the validated string types. The type itself carries the
/// invariant established by `parse`; execution only ever needs the value back
/// as a `&str`.
macro_rules! string_value {
    ($type:ty) => {
        impl $type {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

/// Root of the filesystem whose `zoneinfo` database and `/etc/locale.gen` a
/// value is checked against. Production validates the running system; unit
/// tests validate a fixture so the checks stay enabled on any host instead of
/// being skipped when the host lacks those files.
fn system_root() -> std::path::PathBuf {
    #[cfg(test)]
    {
        system_fixture()
    }

    #[cfg(not(test))]
    {
        std::path::PathBuf::from("/")
    }
}

#[cfg(test)]
fn system_fixture() -> std::path::PathBuf {
    use std::sync::OnceLock;

    static FIXTURE: OnceLock<std::path::PathBuf> = OnceLock::new();

    FIXTURE
        .get_or_init(|| {
            let root = std::env::temp_dir().join("ins-install-plan-system");
            let write = |relative: &str, contents: &str| {
                let path = root.join(relative);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(path, contents);
            };

            write(
                "etc/locale.gen",
                "#en_US.UTF-8 UTF-8\n#de_DE.UTF-8 UTF-8\nC.UTF-8 UTF-8\n",
            );
            for zone in ["UTC", "Europe/Berlin", "America/New_York"] {
                write(&format!("usr/share/zoneinfo/{zone}"), "");
            }

            root
        })
        .clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hostname(String);

impl Hostname {
    pub fn parse(value: &str) -> Result<Self> {
        crate::settings::definitions::system::validate_hostname(value)
            .context("invalid hostname")?;
        Ok(Self(value.to_owned()))
    }
}

string_value!(Hostname);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Username(String);

impl Username {
    pub fn parse(value: &str) -> Result<Self> {
        crate::settings::users::validate_username(value)
            .map_err(|error| anyhow::anyhow!(error.to_string()))
            .context("invalid username")?;
        if value == "root" {
            bail!("Username cannot be 'root'.")
        }
        Ok(Self(value.to_owned()))
    }
}

string_value!(Username);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleKeymap(String);

impl ConsoleKeymap {
    pub fn parse(value: &str) -> Result<Self> {
        validate_relative_resource_name("console keymap", value)?;
        Ok(Self(value.to_owned()))
    }
}

string_value!(ConsoleKeymap);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timezone(String);

impl Timezone {
    pub fn parse(value: &str) -> Result<Self> {
        validate_relative_resource_name("timezone", value)?;
        if !system_root()
            .join("usr/share/zoneinfo")
            .join(value)
            .is_file()
        {
            bail!("unknown timezone {value:?}")
        }
        Ok(Self(value.to_owned()))
    }
}

string_value!(Timezone);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleName(String);

impl LocaleName {
    /// The answer must be an available locale (see `CONTEXT.md`) in the
    /// target's `/etc/locale.gen`: `configure_locale` relies on that to enable
    /// an existing entry rather than appending a new one.
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            bail!("invalid locale {value:?}")
        }
        let locale_gen = std::fs::read_to_string(system_root().join("etc/locale.gen"))
            .context("cannot validate locale without /etc/locale.gen")?;
        if !crate::common::locale_gen::available_locales(&locale_gen)
            .iter()
            .any(|available| available == value)
        {
            bail!("locale {value:?} is not available in /etc/locale.gen")
        }
        Ok(Self(value.to_owned()))
    }
}

string_value!(LocaleName);

#[derive(Clone, PartialEq, Eq)]
pub struct LoginPassword(String);

impl LoginPassword {
    pub fn parse(value: &str) -> Result<Self> {
        validate_secret("login password", value, true)?;
        Ok(Self(value.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LoginPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LoginPassword([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct EncryptionPassword(String);

impl EncryptionPassword {
    pub fn parse(value: &str) -> Result<Self> {
        validate_secret("encryption password", value, false)?;
        Ok(Self(value.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EncryptionPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EncryptionPassword([REDACTED])")
    }
}

fn validate_relative_resource_name(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.chars().any(char::is_whitespace)
        || Path::new(value)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("invalid {label} {value:?}")
    }
    Ok(())
}

fn validate_secret(label: &str, value: &str, reject_colon: bool) -> Result<()> {
    if value.is_empty() {
        bail!("{label} cannot be empty")
    }
    if value.contains(['\n', '\r', '\0']) || (reject_colon && value.contains(':')) {
        bail!("{label} contains a character that cannot be passed safely to the target command")
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskPath(String);

impl DiskPath {
    fn parse(value: &str) -> Result<Self> {
        if !is_safe_device_path(value) {
            bail!("invalid disk path {value:?}; expected a device below /dev")
        }
        Ok(Self(value.to_owned()))
    }
}

string_value!(DiskPath);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionPath(String);

impl PartitionPath {
    fn parse(value: &str) -> Result<Self> {
        if !is_safe_device_path(value) {
            bail!("invalid partition path {value:?}; expected a device below /dev")
        }
        Ok(Self(value.to_owned()))
    }
}

string_value!(PartitionPath);

fn is_safe_device_path(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::RootDir))
        && matches!(components.next(), Some(Component::Normal(component)) if component == "dev")
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.all(|component| matches!(component, Component::Normal(_)))
}

/// A root filesystem ready to be created. Compression belongs to the btrfs
/// variant, so an ext4 filesystem with btrfs compression is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemPlan {
    Btrfs { compression: BtrfsCompression },
    Ext4,
}

impl FilesystemPlan {
    pub fn is_btrfs(self) -> bool {
        matches!(self, Self::Btrfs { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionPlan {
    pub password: EncryptionPassword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DualBootTarget {
    ExistingFreeSpace,
    ResizeAutomatically {
        partition: PartitionPath,
        desired_free_space_bytes: u64,
    },
    ResizedManually {
        partition: PartitionPath,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualPartitions {
    pub root: PartitionPath,
    pub boot: PartitionPath,
    pub swap: Option<PartitionPath>,
    pub home: Option<PartitionPath>,
}

/// The mutually exclusive storage operations available to the installer.
/// Every variant contains all values required by that operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoragePlan {
    Automatic {
        disk: DiskPath,
        filesystem: FilesystemPlan,
        encryption: Option<EncryptionPlan>,
    },
    DualBoot {
        disk: DiskPath,
        filesystem: FilesystemPlan,
        target: DualBootTarget,
    },
    Manual {
        disk: DiskPath,
        filesystem: FilesystemPlan,
        partitions: ManualPartitions,
    },
}

impl StoragePlan {
    pub fn disk(&self) -> &DiskPath {
        match self {
            Self::Automatic { disk, .. }
            | Self::DualBoot { disk, .. }
            | Self::Manual { disk, .. } => disk,
        }
    }

    pub fn filesystem(&self) -> FilesystemPlan {
        match self {
            Self::Automatic { filesystem, .. }
            | Self::DualBoot { filesystem, .. }
            | Self::Manual { filesystem, .. } => *filesystem,
        }
    }

    pub fn encryption(&self) -> Option<&EncryptionPlan> {
        match self {
            Self::Automatic { encryption, .. } => encryption.as_ref(),
            Self::DualBoot { .. } | Self::Manual { .. } => None,
        }
    }
}

/// Desktop and session answers shared by an install plan and `ins arch setup`.
///
/// Both flows read the same steps with the same rules and defaults; keeping the
/// parse in one place is what stops their answers from drifting apart.
/// `autologin_default` is the one legitimate difference: a fresh encrypted
/// install turns autologin on, while setup (which never asks about disk
/// encryption) leaves it off.
#[derive(Debug, Clone, Copy)]
pub struct SessionAnswers {
    pub desktop: DesktopEnvironment,
    pub display_manager: DisplayManager,
    pub use_plymouth: bool,
    pub autologin: bool,
    pub use_xorg: bool,
    pub minimal_mode: bool,
}

impl SessionAnswers {
    pub fn from_context(context: &InstallContext, autologin_default: bool) -> Result<Self> {
        let desktop = context
            .get_answer(&StepId::DesktopEnvironment)
            .map(|answer| {
                DesktopEnvironment::try_from_answer(answer)
                    .with_context(|| format!("invalid desktop environment {answer:?}"))
            })
            .transpose()?
            .unwrap_or(DesktopEnvironment::DEFAULT);
        let display_manager = context
            .get_answer(&StepId::DisplayManager)
            .map(|answer| {
                DisplayManager::try_from_answer(answer)
                    .with_context(|| format!("invalid display manager {answer:?}"))
            })
            .transpose()?
            .unwrap_or(DisplayManager::DEFAULT);

        Ok(Self {
            desktop,
            display_manager,
            use_plymouth: context.bool_answer(StepId::UsePlymouth)?.unwrap_or(true),
            autologin: context
                .bool_answer(StepId::Autologin)?
                .unwrap_or(autologin_default),
            use_xorg: context
                .bool_answer(StepId::UseXorg)?
                .unwrap_or(display_manager == DisplayManager::Lightdm),
            minimal_mode: context.bool_answer(StepId::MinimalMode)?.unwrap_or(false),
        })
    }
}

/// Complete configuration accepted by the execution layer.
///
/// Unlike wizard state, this contains no missing required answers and encodes
/// mutually exclusive installation shapes through enums.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub system_info: SystemInfo,
    pub storage: StoragePlan,
    pub hostname: Hostname,
    pub username: Username,
    pub password: LoginPassword,
    pub keymap: ConsoleKeymap,
    pub timezone: Timezone,
    pub locale: LocaleName,
    pub mirror_region: Option<String>,
    pub kernel: Kernel,
    pub desktop: DesktopEnvironment,
    pub display_manager: DisplayManager,
    pub use_plymouth: bool,
    pub autologin: bool,
    pub use_xorg: bool,
    pub minimal_mode: bool,
}

impl TryFrom<&InstallContext> for InstallPlan {
    type Error = anyhow::Error;

    fn try_from(context: &InstallContext) -> Result<Self> {
        let required = |id| {
            context
                .get_answer(&id)
                .map(String::as_str)
                .with_context(|| format!("missing required answer {id:?}"))
        };
        let disk = DiskPath::parse(required(StepId::Disk)?)?;
        let filesystem = match context.get_answer(&StepId::RootFilesystem) {
            Some(answer) => RootFilesystem::try_from_answer(answer)
                .with_context(|| format!("invalid root filesystem {answer:?}"))?,
            None => RootFilesystem::DEFAULT,
        };
        let filesystem = match filesystem {
            RootFilesystem::Btrfs => {
                let compression = match context.get_answer(&StepId::BtrfsCompression) {
                    Some(answer) => BtrfsCompression::try_from_answer(answer)
                        .with_context(|| format!("invalid btrfs compression {answer:?}"))?,
                    None => BtrfsCompression::DEFAULT,
                };
                FilesystemPlan::Btrfs { compression }
            }
            RootFilesystem::Ext4 => FilesystemPlan::Ext4,
        };

        let partitioning = context.require_partitioning_method()?;
        let storage = match partitioning {
            PartitioningMethod::Automatic => {
                let use_encryption = context.require_bool_answer(StepId::UseEncryption)?;
                let encryption = use_encryption
                    .then(|| {
                        required(StepId::EncryptionPassword).and_then(|password| {
                            Ok(EncryptionPlan {
                                password: EncryptionPassword::parse(password)?,
                            })
                        })
                    })
                    .transpose()?;

                StoragePlan::Automatic {
                    disk,
                    filesystem,
                    encryption,
                }
            }
            PartitioningMethod::DualBoot => {
                if context.bool_answer(StepId::UseEncryption)?.unwrap_or(false) {
                    bail!("dual-boot partitioning cannot use automatic disk encryption")
                }
                let selected = required(StepId::DualBootPartition)?;
                let target = if selected == "__free_space__" {
                    DualBootTarget::ExistingFreeSpace
                } else {
                    let partition = PartitionPath::parse(selected)?;
                    match required(StepId::DualBootInstructions)? {
                        "auto" => {
                            let desired_free_space_bytes = required(StepId::DualBootSize)?
                                .parse::<u64>()
                                .context("dual-boot size is not a byte count")?;
                            DualBootTarget::ResizeAutomatically {
                                partition,
                                desired_free_space_bytes,
                            }
                        }
                        "confirmed" => DualBootTarget::ResizedManually { partition },
                        answer => bail!("invalid dual-boot resize method {answer:?}"),
                    }
                };
                StoragePlan::DualBoot {
                    disk,
                    filesystem,
                    target,
                }
            }
            PartitioningMethod::Manual => {
                if context.bool_answer(StepId::UseEncryption)?.unwrap_or(false) {
                    bail!("manual partitioning cannot use automatic disk encryption")
                }
                StoragePlan::Manual {
                    disk,
                    filesystem,
                    partitions: ManualPartitions {
                        root: PartitionPath::parse(required(StepId::RootPartition)?)?,
                        boot: PartitionPath::parse(required(StepId::BootPartition)?)?,
                        swap: context
                            .get_answer(&StepId::SwapPartition)
                            .map(|value| PartitionPath::parse(value))
                            .transpose()?,
                        home: context
                            .get_answer(&StepId::HomePartition)
                            .map(|value| PartitionPath::parse(value))
                            .transpose()?,
                    },
                }
            }
        };

        let kernel = context.kernel()?;
        // With automatic encryption the boot passphrase already authenticates
        // the user, so autologin is the default rather than an extra prompt.
        let encryption_implies_autologin = matches!(
            storage,
            StoragePlan::Automatic {
                encryption: Some(_),
                ..
            }
        );
        let session = SessionAnswers::from_context(context, encryption_implies_autologin)?;

        Ok(Self {
            system_info: context.system_info.clone(),
            storage,
            hostname: Hostname::parse(required(StepId::Hostname)?)?,
            username: Username::parse(required(StepId::Username)?)?,
            password: LoginPassword::parse(required(StepId::Password)?)?,
            keymap: ConsoleKeymap::parse(required(StepId::Keymap)?)?,
            timezone: Timezone::parse(required(StepId::Timezone)?)?,
            locale: LocaleName::parse(required(StepId::Locale)?)?,
            mirror_region: context.get_answer(&StepId::MirrorRegion).cloned(),
            kernel,
            desktop: session.desktop,
            display_manager: session.display_manager,
            use_plymouth: session.use_plymouth,
            autologin: session.autologin,
            use_xorg: session.use_xorg,
            minimal_mode: session.minimal_mode,
        })
    }
}

impl InstallPlan {
    pub fn boot_mode(&self) -> &BootMode {
        &self.system_info.boot_mode
    }
}

#[cfg(test)]
pub(crate) fn test_install_plan() -> InstallPlan {
    InstallPlan {
        system_info: SystemInfo {
            boot_mode: BootMode::UEFI64,
            ..SystemInfo::default()
        },
        storage: StoragePlan::Automatic {
            disk: DiskPath("/dev/test-disk".to_owned()),
            filesystem: FilesystemPlan::Ext4,
            encryption: None,
        },
        hostname: Hostname::parse("test-host").unwrap(),
        username: Username::parse("test-user").unwrap(),
        password: LoginPassword::parse("test-password").unwrap(),
        keymap: ConsoleKeymap::parse("us").unwrap(),
        timezone: Timezone::parse("UTC").unwrap(),
        locale: LocaleName::parse("en_US.UTF-8").unwrap(),
        mirror_region: None,
        kernel: Kernel::Linux,
        desktop: DesktopEnvironment::Tty,
        display_manager: DisplayManager::Gdm,
        use_plymouth: false,
        autologin: false,
        use_xorg: false,
        minimal_mode: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn required_context(partitioning: &str) -> InstallContext {
        let mut context = InstallContext::new();
        for (id, answer) in [
            (StepId::Disk, "/dev/sda"),
            (StepId::PartitioningMethod, partitioning),
            (StepId::Hostname, "instant-test"),
            (StepId::Username, "tester"),
            (StepId::Password, "correct horse battery staple"),
            (StepId::Keymap, "us"),
            (StepId::Timezone, "UTC"),
            (StepId::Locale, "en_US.UTF-8"),
            (StepId::UseEncryption, "no"),
        ] {
            context.set_answer(id, answer.to_owned());
        }
        context
    }

    #[test]
    fn btrfs_plan_owns_its_compression() {
        let mut context = required_context("automatic");
        context.set_answer(StepId::RootFilesystem, "btrfs".to_owned());
        context.set_answer(StepId::BtrfsCompression, "lzo".to_owned());

        let plan = InstallPlan::try_from(&context).unwrap();

        assert_eq!(
            plan.storage.filesystem(),
            FilesystemPlan::Btrfs {
                compression: BtrfsCompression::Lzo
            }
        );
    }

    #[test]
    fn ext4_plan_cannot_carry_btrfs_compression() {
        let mut context = required_context("automatic");
        context.set_answer(StepId::RootFilesystem, "ext4".to_owned());
        context.set_answer(StepId::BtrfsCompression, "zlib".to_owned());

        let plan = InstallPlan::try_from(&context).unwrap();

        assert_eq!(plan.storage.filesystem(), FilesystemPlan::Ext4);
    }

    #[test]
    fn manual_plan_requires_structurally_complete_partitions() {
        let context = required_context("manual");
        let error = InstallPlan::try_from(&context).unwrap_err();
        assert!(error.to_string().contains("RootPartition"));

        let mut context = context;
        context.set_answer(StepId::RootPartition, "/dev/sda2".to_owned());
        context.set_answer(StepId::BootPartition, "/dev/sda1".to_owned());
        let plan = InstallPlan::try_from(&context).unwrap();
        assert!(matches!(plan.storage, StoragePlan::Manual { .. }));
    }

    #[test]
    fn display_labels_are_not_accepted_as_partitioning_values() {
        let context = required_context("Automatic (Erase Disk)");
        let error = InstallPlan::try_from(&context).unwrap_err();
        assert!(error.to_string().contains("invalid partitioning method"));
    }

    #[test]
    fn encryption_cannot_leak_into_dual_boot() {
        let mut context = required_context("dual_boot");
        context.set_answer(StepId::UseEncryption, "yes".to_owned());
        context.set_answer(StepId::EncryptionPassword, "secret".to_owned());
        context.set_answer(StepId::DualBootPartition, "__free_space__".to_owned());

        let error = InstallPlan::try_from(&context).unwrap_err();
        assert!(error.to_string().contains("dual-boot"));
    }

    #[test]
    fn automatic_plan_requires_an_explicit_encryption_choice() {
        let mut context = required_context("automatic");
        context.answers.remove(&StepId::UseEncryption);

        let error = InstallPlan::try_from(&context).unwrap_err();
        assert!(error.to_string().contains("UseEncryption"));
    }

    #[test]
    fn validated_scalars_reject_unsafe_or_unknown_values() {
        assert!(Hostname::parse("-bad-hostname").is_err());
        assert!(Username::parse("root").is_err());
        assert!(ConsoleKeymap::parse("../etc/passwd").is_err());
        assert!(Timezone::parse("../etc/passwd").is_err());
        // The zoneinfo and locale.gen checks run against the test fixture, so
        // unknown values are still rejected without depending on the host.
        assert!(Timezone::parse("Not/AZone").is_err());
        assert!(LocaleName::parse("not_a_real_LOCALE.UTF-8").is_err());
        assert!(LoginPassword::parse("safe\nroot:changed").is_err());
        assert!(LoginPassword::parse("contains:delimiter").is_err());
        assert!(EncryptionPassword::parse("").is_err());
        assert!(DiskPath::parse("/dev/../etc/passwd").is_err());
        assert!(PartitionPath::parse("/dev/sda1/../sda2").is_err());
    }

    #[test]
    fn system_backed_checks_consult_the_test_fixture() {
        // `Timezone::parse` and `LocaleName::parse` check the fixture root in
        // test builds. If that seam stopped working, `Pacific/Auckland` (in
        // the host's zoneinfo, absent from the fixture) would be accepted.
        assert!(Timezone::parse("UTC").is_ok());
        assert!(Timezone::parse("Pacific/Auckland").is_err());
        assert!(LocaleName::parse("de_DE.UTF-8").is_ok());
    }

    #[test]
    fn secret_debug_output_is_redacted() {
        let login = LoginPassword::parse("login-secret").unwrap();
        let encryption = EncryptionPassword::parse("encryption-secret").unwrap();

        assert!(!format!("{login:?}").contains("login-secret"));
        assert!(!format!("{encryption:?}").contains("encryption-secret"));
    }
}
