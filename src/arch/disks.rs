use anyhow::Result;
use serde_json::Value;
use std::process::Command;

use crate::arch::engine::DataKey;

/// Get the current root filesystem device (e.g., /dev/mapper/vg-root, /dev/sda2)
pub fn get_root_device() -> Result<Option<String>> {
    let output = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "/"])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let root_device = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root_device.is_empty() {
        return Ok(None);
    }

    Ok(Some(root_device))
}

/// Get the physical disk that contains the current root filesystem
pub fn get_boot_disk() -> Result<Option<String>> {
    // First, get the root filesystem device
    let root_device = match get_root_device()? {
        Some(device) => device,
        None => return Ok(None),
    };

    // Get the full block device hierarchy as JSON to trace back to physical disk
    let lsblk_output = Command::new("lsblk").args(["-J"]).output()?;
    if !lsblk_output.status.success() {
        return Ok(None);
    }

    let lsblk_json: Value = serde_json::from_slice(&lsblk_output.stdout)?;

    // Function to recursively find the physical disk for a given device
    fn find_physical_disk(blockdevices: &[Value], target_name: &str) -> Option<String> {
        for device in blockdevices {
            let name = device.get("name")?.as_str()?;
            let device_type = device.get("type")?.as_str()?;

            // Convert name to full path if it doesn't contain /
            let device_path = if name.contains('/') {
                name.to_string()
            } else {
                format!("/dev/{}", name)
            };

            // If this device matches our target, trace it up to the physical disk
            if device_path == target_name || name == target_name.trim_start_matches("/dev/") {
                if device_type == "disk" {
                    return Some(device_path);
                }

                // If it's not a disk, look for parent by checking children in reverse
                // We need to find which disk contains this device
                return find_parent_disk(blockdevices, name);
            }

            // Recursively check children
            if let Some(children) = device.get("children").and_then(|c| c.as_array())
                && let Some(disk) = find_physical_disk(children, target_name)
            {
                return Some(disk);
            }
        }
        None
    }

    // Function to find which disk contains a given partition/volume
    fn find_parent_disk(blockdevices: &[Value], target_name: &str) -> Option<String> {
        for device in blockdevices {
            let device_type = device.get("type")?.as_str()?;

            if device_type == "disk" {
                // Check if this disk contains the target
                if let Some(children) = device.get("children").and_then(|c| c.as_array())
                    && contains_device(children, target_name)
                {
                    let name = device.get("name")?.as_str()?;
                    let disk_path = if name.contains('/') {
                        name.to_string()
                    } else {
                        format!("/dev/{}", name)
                    };
                    return Some(disk_path);
                }
            } else if let Some(children) = device.get("children").and_then(|c| c.as_array())
                && let Some(disk) = find_parent_disk(children, target_name)
            {
                return Some(disk);
            }
        }
        None
    }

    // Function to check if a list of children contains the target device (recursively)
    fn contains_device(children: &[Value], target_name: &str) -> bool {
        for child in children {
            if let Some(name) = child.get("name").and_then(|n| n.as_str())
                && name == target_name
            {
                return true;
            }
            if let Some(grandchildren) = child.get("children").and_then(|c| c.as_array())
                && contains_device(grandchildren, target_name)
            {
                return true;
            }
        }
        false
    }

    if let Some(blockdevices) = lsblk_json.get("blockdevices").and_then(|b| b.as_array()) {
        Ok(find_physical_disk(blockdevices, &root_device))
    } else {
        Ok(None)
    }
}

/// Get list of mounted partitions on the given disk
pub fn get_mounted_partitions(disk: &str) -> Result<Vec<String>> {
    let output = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE"])
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut mounted = Vec::new();

    for line in stdout.lines() {
        let source = line.trim();
        if source.starts_with(disk) {
            mounted.push(source.to_string());
        }
    }

    Ok(mounted)
}

/// Get list of swap partitions on the given disk
pub fn get_swap_partitions(disk: &str) -> Result<Vec<String>> {
    let swaps = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
    let mut swap_parts = Vec::new();

    for line in swaps.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(filename) = parts.first()
            && filename.starts_with(disk)
        {
            swap_parts.push(filename.to_string());
        }
    }

    Ok(swap_parts)
}

/// Result of preparing a disk for installation
#[derive(Debug, Default)]
pub struct DiskPrepareResult {
    pub unmounted: Vec<String>,
    pub swapoff: Vec<String>,
}

impl DiskPrepareResult {
    pub fn is_empty(&self) -> bool {
        self.unmounted.is_empty() && self.swapoff.is_empty()
    }

    pub fn total_count(&self) -> usize {
        self.unmounted.len() + self.swapoff.len()
    }
}

/// Prepare a disk for installation by unmounting all partitions and disabling swap
pub fn prepare_disk(disk: &str) -> Result<DiskPrepareResult> {
    let mut result = DiskPrepareResult::default();

    // Unmount all mounted partitions
    for partition in get_mounted_partitions(disk)? {
        let status = Command::new("umount").arg(&partition).status()?;
        if status.success() {
            result.unmounted.push(partition);
        } else {
            anyhow::bail!("Failed to unmount {}", partition);
        }
    }

    // Disable swap on all swap partitions
    for partition in get_swap_partitions(disk)? {
        let status = Command::new("swapoff").arg(&partition).status()?;
        if status.success() {
            result.swapoff.push(partition);
        } else {
            anyhow::bail!("Failed to swapoff {}", partition);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_root_device() {
        // This test will only work on a running Linux system
        // It should be able to find the root device
        if cfg!(target_os = "linux") {
            let result = get_root_device();
            assert!(result.is_ok());

            match result.unwrap() {
                Some(device) => {
                    // In containers, the root device might be "overlay" or similar, so we don't assert /dev/ prefix
                    assert!(!device.is_empty());
                    println!("Root device detected: {}", device);

                    // Most common formats start with /dev/, but some environments
                    // (containers, network mounts, etc.) may return different formats
                    // The important thing is that we get a valid device identifier
                }
                None => {
                    // This might happen in some container environments
                    println!("No root device detected (might be normal in containers)");
                }
            }
        }
    }

    #[test]
    fn test_get_boot_disk() {
        // This test will only work on a running Linux system
        if cfg!(target_os = "linux") {
            let result = get_boot_disk();
            assert!(result.is_ok());

            match result.unwrap() {
                Some(disk) => {
                    assert!(disk.starts_with("/dev/"));
                    assert!(!disk.contains("mapper")); // Should be a physical disk, not logical volume
                    println!("Boot disk detected: {}", disk);
                }
                None => {
                    // This might happen in some container environments
                    println!("No boot disk detected (might be normal in containers)");
                }
            }
        }
    }
}

/// Represents a disk entry with path and size information
#[derive(Clone, Debug)]
pub struct DiskEntry {
    /// Device path (e.g., /dev/sda)
    pub path: String,
    /// Human-readable size (e.g., "500 GiB")
    pub size: String,
}

impl DiskEntry {
    pub fn new(path: String, size: String) -> Self {
        Self { path, size }
    }
}

impl crate::menu_utils::FzfSelectable for DiskEntry {
    fn fzf_display_text(&self) -> String {
        format!("{} ({})", self.path, self.size)
    }
}

pub struct DisksKey;

impl DataKey for DisksKey {
    type Value = Vec<DiskEntry>;
    const KEY: &'static str = "disks";
}

pub struct DiskProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for DiskProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        // Run fdisk -l
        // We assume the process is already running as root (enforced in CLI)
        let output = Command::new("fdisk").arg("-l").output()?;

        if !output.status.success() {
            eprintln!(
                "Failed to list disks: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut disks = Vec::new();

        // Parse output: look for lines starting with "Disk /dev/..."
        // Example: Disk /dev/nvme0n1: 476.94 GiB, 512110190592 bytes, 1000215216 sectors
        for line in stdout.lines() {
            if line.starts_with("Disk /dev/") && line.contains(':') {
                // Filter out loopback devices (/dev/loop*)
                if line.contains("/dev/loop") {
                    continue;
                }
                // Extract the part before the comma usually, or just the whole line up to size
                // "Disk /dev/sda: 500 GiB, ..."
                // We want to present something like "/dev/sda (500 GiB)"

                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 {
                    let dev_path = parts[0]
                        .trim()
                        .strip_prefix("Disk ")
                        .unwrap_or(parts[0].trim());
                    let details = parts[1].trim();
                    // details might be "476.94 GiB, 512110190592 bytes, 1000215216 sectors"
                    // We just want the first part "476.94 GiB"
                    let size = details.split(',').next().unwrap_or(details).trim();

                    disks.push(DiskEntry::new(dev_path.to_string(), size.to_string()));
                }
            }
        }

        if disks.is_empty() {
            // Fallback or warning?
            // Maybe we are not root?
            eprintln!("No disks found. Are you running with sudo?");
        }

        context.set::<DisksKey>(disks);

        Ok(())
    }
}
