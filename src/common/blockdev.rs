use std::fmt;
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::process::Command;

/// A filesystem, named exactly as `lsblk` and `blkid` spell it.
///
/// The name is kept verbatim because it has to round-trip: it is read from
/// `lsblk`, passed to `mount -t`, written into `fstab`, and compared against
/// what is already on disk. A closed enum would have to map back to a string at
/// each of those, with a fallback for the unknown case — and that fallback is
/// where a typo silently becomes a wrong mount. So the string stays, and what
/// the code knows *about* filesystems lives here as named methods rather than as
/// string comparisons scattered across callers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Filesystem(String);

/// Whether, and how, a filesystem can be made smaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShrinkSupport {
    /// No supported tool for this filesystem.
    Unsupported,
    /// Shrinkable, but only once unmounted.
    RequiresUnmount { tool: ShrinkTool },
    /// Shrinkable while still mounted. Relocates data, so it is I/O intensive.
    WhileMounted { tool: ShrinkTool },
}

impl ShrinkSupport {
    /// Whether this filesystem can be shrunk at all.
    pub fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    /// The tool that performs the shrink, if there is one.
    pub fn tool(self) -> Option<ShrinkTool> {
        match self {
            Self::Unsupported => None,
            Self::RequiresUnmount { tool } | Self::WhileMounted { tool } => Some(tool),
        }
    }

    /// Whether the filesystem must be unmounted before shrinking.
    pub fn requires_unmount(self) -> bool {
        matches!(self, Self::RequiresUnmount { .. })
    }
}

/// The tool that shrinks a given filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShrinkTool {
    /// `ntfsresize`, for NTFS.
    NtfsResize,
    /// `resize2fs`, for the ext family.
    Resize2Fs,
    /// `btrfs filesystem resize`, which works on a mounted filesystem.
    Btrfs,
}

impl Filesystem {
    /// Name a filesystem as `lsblk` or `blkid` reports it.
    ///
    /// Comparison is case-insensitive throughout, because the same filesystem is
    /// spelled `crypto_LUKS` by one tool and `crypt_luks` by another, and `vfat`
    /// versus `VFAT` by kernel version.
    pub fn parse(name: &str) -> Self {
        Self(name.trim().to_owned())
    }

    /// The name, exactly as it was reported.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn matches(&self, name: &str) -> bool {
        self.0.eq_ignore_ascii_case(name)
    }

    fn matches_any(&self, names: &[&str]) -> bool {
        names.iter().any(|name| self.matches(name))
    }

    /// A Linux root filesystem we recognise: the ones the installer is willing
    /// to treat as "the running system" or as a shrinkable Linux partition.
    pub fn is_linux_root(&self) -> bool {
        self.matches_any(&["ext2", "ext3", "ext4", "btrfs", "xfs"])
    }

    /// The ext family, which shares one set of tools and one resize path.
    pub fn is_ext(&self) -> bool {
        self.matches_any(&["ext2", "ext3", "ext4"])
    }

    /// NTFS, which has its own resize tool and its own safety rules.
    pub fn matches_ntfs(&self) -> bool {
        self.matches("ntfs")
    }

    /// btrfs, which is the one filesystem here that is organised into
    /// subvolumes and so needs one named when mounting a known layout.
    pub fn is_btrfs(&self) -> bool {
        self.matches("btrfs")
    }

    /// An encrypted container, as opposed to a plain filesystem.
    pub fn is_encrypted(&self) -> bool {
        self.matches_any(&["crypto_luks", "crypt_luks", "luks"])
    }

    /// The filesystem an EFI system partition is formatted with.
    pub fn is_esp(&self) -> bool {
        self.matches_any(&["vfat", "fat", "fat12", "fat16", "fat32", "msdos"])
    }

    /// Options needed to mount this filesystem read-only for inspection.
    ///
    /// ext3 and ext4 replay the journal on a read-only mount unless told not
    /// to, and xfs does the same with its log. btrfs needs a subvolume *named*
    /// rather than an option here — the name is an installer constant, so the
    /// caller adds it.
    pub fn read_only_options(&self) -> &'static [&'static str] {
        if self.matches_any(&["ext3", "ext4"]) {
            &["noload"]
        } else if self.matches("xfs") {
            &["norecovery"]
        } else {
            &[]
        }
    }

    /// Whether this filesystem can be swapped in place, for a dual-boot resize.
    ///
    /// btrfs is the only one that shrinks while mounted; NTFS and the ext family
    /// need the filesystem unmounted first. Both the resize dispatcher and the
    /// automatic resize path read their tool from this answer, so they cannot
    /// disagree about which filesystems are supported.
    pub fn shrink_support(&self) -> ShrinkSupport {
        if self.matches("ntfs") {
            ShrinkSupport::RequiresUnmount {
                tool: ShrinkTool::NtfsResize,
            }
        } else if self.matches_any(&["ext2", "ext3", "ext4"]) {
            ShrinkSupport::RequiresUnmount {
                tool: ShrinkTool::Resize2Fs,
            }
        } else if self.matches("btrfs") {
            ShrinkSupport::WhileMounted {
                tool: ShrinkTool::Btrfs,
            }
        } else {
            // XFS cannot shrink at all; everything else has no tool here.
            ShrinkSupport::Unsupported
        }
    }
}

impl From<&str> for Filesystem {
    fn from(name: &str) -> Self {
        Self::parse(name)
    }
}

impl fmt::Display for Filesystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Filesystem {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LsblkOutput {
    pub blockdevices: Vec<BlockDevice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockDevice {
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub fstype: Option<Filesystem>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub mountpoint: Option<String>,
    #[serde(default)]
    pub pttype: Option<String>,
    #[serde(default)]
    pub parttype: Option<String>,
    #[serde(default)]
    pub children: Vec<BlockDevice>,
}

impl BlockDevice {
    pub fn path(&self) -> String {
        if self.name.starts_with('/') {
            self.name.clone()
        } else {
            format!("/dev/{}", self.name)
        }
    }

    pub fn is_disk(&self) -> bool {
        self.device_type == "disk"
    }

    pub fn is_partition(&self) -> bool {
        self.device_type == "part"
    }

    /// An encrypted container, as opposed to a plain filesystem.
    pub fn is_luks(&self) -> bool {
        self.fstype.as_ref().is_some_and(Filesystem::is_encrypted)
    }

    /// A Linux root filesystem: the ext family, btrfs or xfs.
    pub fn is_linux_root_fs(&self) -> bool {
        self.fstype.as_ref().is_some_and(Filesystem::is_linux_root)
    }

    /// A partition holding a Linux root filesystem.
    ///
    /// Both halves are required: a Linux filesystem on a whole disk is not a
    /// root partition, and a partition with no Linux filesystem is not one
    /// either. Callers asking this question want both, so they are one call.
    pub fn is_linux_root_partition(&self) -> bool {
        self.is_partition() && self.is_linux_root_fs()
    }

    /// An EFI system partition, recognised either by its filesystem or by its
    /// partition-table type — a blank ESP has the right type code before
    /// anything is formatted into it.
    ///
    /// The two conditions are separate facts about separate fields, so the
    /// question belongs here rather than at a call site. The partition guard
    /// belongs with it rather than beside it, because every caller asking "is
    /// this the ESP" wants the partition, and one name for one fact beats a
    /// guard each caller has to remember.
    pub fn is_esp_partition(&self) -> bool {
        self.is_partition()
            && (self.fstype.as_ref().is_some_and(Filesystem::is_esp)
                || self.parttype.as_deref().is_some_and(is_efi_partition_type))
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "type": self.device_type,
            "size": self.size,
            "fstype": self.fstype,
            "uuid": self.uuid,
            "label": self.label,
            "mountpoint": self.mountpoint,
            "pttype": self.pttype,
            "parttype": self.parttype,
        })
    }
}

pub fn load_lsblk(extra_args: &[&str]) -> Result<LsblkOutput> {
    let mut cmd = Command::new("lsblk");
    cmd.args([
        "-J",
        "-b",
        "-o",
        "NAME,SIZE,TYPE,FSTYPE,UUID,LABEL,MOUNTPOINT,PTTYPE,PARTTYPE",
    ]);
    cmd.args(extra_args);

    let output = cmd.output().context("Failed to run lsblk")?;

    if !output.status.success() {
        bail!("lsblk failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse lsblk JSON")
}

/// What lsblk reports about a device path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// A whole disk.
    Disk,
    /// A partition of a disk.
    Partition,
    /// Not listed, or listed as neither — an LVM mapper, a RAID member, a ZFS
    /// pool. These are legitimate install targets, so this carries no
    /// objection either way.
    Unknown,
}

/// Classify `path` against a parsed lsblk tree, matching on device path so
/// `/dev/sda` and a name of `sda` resolve to the same entry.
pub fn classify_device(path: &str, output: &LsblkOutput) -> DeviceKind {
    fn search(device: &BlockDevice, path: &str) -> Option<DeviceKind> {
        if device.path() == path {
            return Some(match device.device_type.as_str() {
                "disk" => DeviceKind::Disk,
                "part" => DeviceKind::Partition,
                _ => DeviceKind::Unknown,
            });
        }
        device.children.iter().find_map(|child| search(child, path))
    }

    output
        .blockdevices
        .iter()
        .find_map(|device| search(device, path))
        .unwrap_or(DeviceKind::Unknown)
}

/// The device tree, read once per process.
///
/// Plan construction asks about several device paths in a row and the tree does
/// not change while it does, so this is a cache and not a per-call `lsblk`.
pub fn device_tree() -> Result<&'static LsblkOutput> {
    static TREE: OnceLock<Option<LsblkOutput>> = OnceLock::new();
    TREE.get_or_init(|| load_lsblk(&[]).ok())
        .as_ref()
        .ok_or_else(|| anyhow!("could not read the device tree with lsblk"))
}

/// Classify a device path against the live lsblk tree.
///
/// A tree that cannot be read yields [`DeviceKind::Unknown`] rather than an
/// error: refusing an install because `lsblk` was unavailable would be a worse
/// outcome than accepting one whose kind is unverified.
pub fn classify_device_path(path: &str) -> DeviceKind {
    match device_tree() {
        Ok(tree) => classify_device(path, tree),
        Err(_) => DeviceKind::Unknown,
    }
}

/// Ask `blkid` which filesystem is on `device`.
///
/// Shared by the installer-identity probe and `ins dev chroot`, which both need
/// the answer before they can mount a device read-only.
pub fn blkid_filesystem(device: &str) -> Result<Option<Filesystem>> {
    let output = Command::new("blkid")
        .args(["-o", "value", "-s", "TYPE", device])
        .output()
        .with_context(|| format!("Failed to run blkid on {device}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!value.is_empty()).then(|| Filesystem::parse(&value)))
}

/// A partition-table type code identifying an EFI system partition.
///
/// This is a *partition* type, not a filesystem, so it stays a free function:
/// the vocabulary is a GUID for GPT and a one-byte code for MBR, and it belongs
/// to neither filesystem nor disk.
pub fn is_efi_partition_type(parttype: &str) -> bool {
    let normalized = parttype.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "0xef" | "ef" | "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_device_paths() {
        let device = BlockDevice {
            name: "sda1".to_string(),
            device_type: "part".to_string(),
            size: None,
            fstype: None,
            uuid: None,
            label: None,
            mountpoint: None,
            pttype: None,
            parttype: None,
            children: Vec::new(),
        };

        assert_eq!(device.path(), "/dev/sda1");
    }

    #[test]
    fn detects_efi_partition_types() {
        assert!(is_efi_partition_type("0xEF"));
        assert!(is_efi_partition_type(
            "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
        ));
        assert!(!is_efi_partition_type("0x83"));
    }

    #[test]
    fn detects_linux_root_filesystems() {
        assert!(Filesystem::parse("ext4").is_linux_root());
        assert!(Filesystem::parse("btrfs").is_linux_root());
        assert!(!Filesystem::parse("vfat").is_linux_root());
    }

    #[test]
    fn filesystem_names_compare_case_insensitively() {
        // The same filesystem is spelled crypto_LUKS by one tool and
        // crypt_luks by another, and vfat/VFAT by kernel version.
        assert!(Filesystem::parse("crypto_LUKS").is_encrypted());
        assert!(Filesystem::parse("CRYPT_LUKS").is_encrypted());
        assert!(Filesystem::parse("VFAT").is_esp());
    }

    #[test]
    fn the_name_round_trips_exactly() {
        // It has to survive into `mount -t` and fstab unchanged.
        let fs = Filesystem::parse("  crypto_LUKS  ");
        assert_eq!(fs.as_str(), "crypto_LUKS");
        assert_eq!(fs.to_string(), "crypto_LUKS");
    }

    #[test]
    fn classifies_disks_partitions_and_anything_else() {
        let tree: LsblkOutput = serde_json::from_str(
            r#"{"blockdevices":[{
                "name":"sda","type":"disk","children":[
                  {"name":"sda1","type":"part","fstype":"vfat"},
                  {"name":"sda2","type":"part","fstype":"ext4","children":[
                    {"name":"mapper/sda2-crypt","type":"crypt"},
                    {"name":"mapper/vg-root","type":"lvm","fstype":"ext4"}
                  ]}
                ]},
                {"name":"dm-0","type":"lvm"}]}"#,
        )
        .expect("valid lsblk json");

        assert_eq!(classify_device("/dev/sda", &tree), DeviceKind::Disk);
        assert_eq!(classify_device("/dev/sda1", &tree), DeviceKind::Partition);
        // Nested devices are reachable, so an LVM root on a partition is not
        // mistaken for something below /dev that nobody knows about.
        assert_eq!(
            classify_device("/dev/mapper/vg-root", &tree),
            DeviceKind::Unknown
        );
        // A path lsblk does not list at all.
        assert_eq!(classify_device("/dev/sdz9", &tree), DeviceKind::Unknown);
    }

    #[test]
    fn only_btrfs_shrinks_while_mounted() {
        assert_eq!(
            Filesystem::parse("btrfs").shrink_support(),
            ShrinkSupport::WhileMounted {
                tool: ShrinkTool::Btrfs
            }
        );
        assert_eq!(
            Filesystem::parse("ntfs").shrink_support(),
            ShrinkSupport::RequiresUnmount {
                tool: ShrinkTool::NtfsResize
            }
        );
        assert_eq!(
            Filesystem::parse("ext4").shrink_support(),
            ShrinkSupport::RequiresUnmount {
                tool: ShrinkTool::Resize2Fs
            }
        );
    }

    #[test]
    fn xfs_and_the_unknown_cannot_be_shrunk() {
        // XFS has no shrink at all, which is why an online resize of an XFS
        // root is impossible rather than merely awkward.
        for name in ["xfs", "vfat", "zfs", ""] {
            let support = Filesystem::parse(name).shrink_support();
            assert_eq!(support, ShrinkSupport::Unsupported, "{name}");
            assert!(!support.is_supported());
            assert!(!support.requires_unmount());
            assert_eq!(support.tool(), None);
        }
    }

    /// A device of the given type, with the given filesystem name and
    /// partition type code.
    fn device(device_type: &str, fs: Option<&str>, parttype: Option<&str>) -> BlockDevice {
        BlockDevice {
            name: "vda1".to_string(),
            device_type: device_type.to_string(),
            size: Some(1024 * 1024 * 1024),
            fstype: fs.map(Filesystem::parse),
            uuid: None,
            label: None,
            mountpoint: None,
            pttype: None,
            parttype: parttype.map(str::to_string),
            children: Vec::new(),
        }
    }

    const ESP_GUID: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";

    #[test]
    fn a_linux_root_partition_is_a_partition_with_a_linux_filesystem() {
        assert!(device("part", Some("btrfs"), None).is_linux_root_partition());
        assert!(device("part", Some("ext4"), None).is_linux_root_partition());
        assert!(!device("part", Some("vfat"), None).is_linux_root_partition());
        assert!(!device("part", None, None).is_linux_root_partition());
        // A whole disk whose filesystem happens to be ext4 is not a partition.
        assert!(!device("disk", Some("ext4"), None).is_linux_root_partition());
    }

    #[test]
    fn an_esp_partition_is_a_partition_recognised_either_way() {
        // By filesystem, as an ESP ends up once formatted.
        assert!(device("part", Some("vfat"), None).is_esp_partition());
        // By partition type code, before anything is written into it.
        assert!(device("part", None, Some(ESP_GUID)).is_esp_partition());
        // A whole disk is not a partition, however it is typed.
        assert!(!device("disk", Some("vfat"), None).is_esp_partition());
        assert!(!device("part", Some("ext4"), None).is_esp_partition());
    }
}
