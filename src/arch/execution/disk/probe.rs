use anyhow::{Context, Result};

pub fn get_total_ram_gb() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: u64 = parts[1].parse().ok()?;
                return Some(kb.div_ceil(1024 * 1024));
            }
        }
    }
    None
}

pub fn get_partition_size_bytes(device_path: &str) -> Result<u64> {
    let output = std::process::Command::new("lsblk")
        .args(["-n", "-o", "SIZE", "-b", device_path])
        .output()
        .context("Failed to get partition size")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse()
        .context("Failed to parse partition size")
}

pub fn get_current_partitions(disk_path: &str) -> Result<std::collections::HashSet<String>> {
    let output = std::process::Command::new("lsblk")
        .args(["-n", "-o", "NAME", "-r", disk_path])
        .output()
        .context("Failed to run lsblk")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let disk_name = disk_path.strip_prefix("/dev/").unwrap_or(disk_path);

    let partitions: std::collections::HashSet<String> = stdout
        .lines()
        .filter(|l| l.starts_with(disk_name))
        .filter(|l| *l != disk_name)
        .map(|name| format!("/dev/{}", name))
        .collect();

    Ok(partitions)
}
