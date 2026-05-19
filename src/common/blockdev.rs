use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::process::Command;

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
    pub fstype: Option<String>,
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

    pub fn is_luks(&self) -> bool {
        self.fstype
            .as_deref()
            .is_some_and(|fs| fs.eq_ignore_ascii_case("crypto_LUKS"))
    }

    pub fn is_linux_root_fs(&self) -> bool {
        self.fstype.as_deref().is_some_and(is_linux_root_fs)
    }

    pub fn is_efi(&self) -> bool {
        self.fstype
            .as_deref()
            .is_some_and(|fs| fs.eq_ignore_ascii_case("vfat"))
            || self.parttype.as_deref().is_some_and(is_efi_partition_type)
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

pub fn is_linux_root_fs(fs: &str) -> bool {
    matches!(
        fs.to_ascii_lowercase().as_str(),
        "ext2" | "ext3" | "ext4" | "btrfs" | "xfs"
    )
}

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
        assert!(is_linux_root_fs("ext4"));
        assert!(is_linux_root_fs("btrfs"));
        assert!(!is_linux_root_fs("vfat"));
    }
}
