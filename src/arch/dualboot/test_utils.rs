#[cfg(test)]
use std::io::Write;
#[cfg(test)]
use std::process::Command;
#[cfg(test)]
use tempfile::NamedTempFile;

#[cfg(test)]
pub const MB: u64 = 1024 * 1024;

#[cfg(test)]
pub struct TestDisk {
    pub path: tempfile::TempPath,
    pub size_mb: u64,
}

#[cfg(test)]
impl TestDisk {
    /// Create a new empty disk image of specified size in MB
    pub fn new(size_mb: u64) -> Self {
        let file = NamedTempFile::new().expect("Failed to create temp image");
        file.as_file()
            .set_len(size_mb * MB)
            .expect("Failed to set image size");

        Self {
            path: file.into_temp_path(),
            size_mb,
        }
    }

    /// Run sfdisk with a script to partition the disk
    pub fn partition(&self, script: &str) {
        let mut child = Command::new("sfdisk")
            .arg("--no-reread")
            .arg("--no-tell-kernel")
            .arg("--quiet")
            .arg(self.path_str())
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn sfdisk");

        child
            .stdin
            .as_mut()
            .expect("Failed to open stdin")
            .write_all(script.as_bytes())
            .expect("Failed to write script to sfdisk");

        let status = child.wait().expect("Failed to wait for sfdisk");
        if !status.success() {
            panic!("sfdisk failed with status: {}", status);
        }
    }

    pub fn path_str(&self) -> &str {
        self.path.to_str().expect("Valid path")
    }

    /// Format a partition on the disk image (or the whole disk if it's a partition)
    pub fn format_ext4(&self) {
        let status = Command::new("mkfs.ext4")
            .arg("-F")
            .arg(self.path_str())
            .status()
            .expect("Failed to run mkfs.ext4");
        assert!(status.success(), "mkfs.ext4 failed");
    }

    /// Format as NTFS
    pub fn format_ntfs(&self) {
        let status = Command::new("mkfs.ntfs")
            .arg("-F")
            .arg("-Q") // Quick format
            .arg(self.path_str())
            .status()
            .expect("Failed to run mkfs.ntfs");
        assert!(status.success(), "mkfs.ntfs failed");
    }
}

/// Common GPT script for a dual-boot-like scenario
#[cfg(test)]
pub const GPT_DUAL_BOOT_SCRIPT: &str = "label: gpt
size=100M, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name=\"EFI\"
size=500M, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name=\"Linux\"
size=1G, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name=\"Windows\"
";

/// Common MBR script
#[cfg(test)]
pub const MBR_SCRIPT: &str = "label: dos
size=100M, type=83, bootable
size=500M, type=83
";
