use anyhow::Result;
use std::process::Command;

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

                    disks.push(format!("{} ({})", dev_path, size));
                }
            }
        }

        if disks.is_empty() {
            // Fallback or warning?
            // Maybe we are not root?
            eprintln!("No disks found. Are you running with sudo?");
        }

        let mut data = context.data.lock().unwrap();
        data.insert("disks".to_string(), disks.join("\n"));

        Ok(())
    }
}
