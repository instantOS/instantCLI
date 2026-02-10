use anyhow::{Context, Result};

pub fn get_part_path(disk: &str, part_num: u32) -> String {
    if disk.chars().last().unwrap_or(' ').is_numeric() {
        format!("{}p{}", disk, part_num)
    } else {
        format!("{}{}", disk, part_num)
    }
}

pub fn parse_partition_number(disk_path: &str, partition_path: &str) -> Result<u32> {
    let disk_name = disk_path.strip_prefix("/dev/").unwrap_or(disk_path);
    let part_name = partition_path
        .strip_prefix("/dev/")
        .unwrap_or(partition_path);

    if !part_name.starts_with(disk_name) {
        anyhow::bail!(
            "Partition {} does not belong to disk {}",
            partition_path,
            disk_path
        );
    }

    let suffix = &part_name[disk_name.len()..];
    let suffix = suffix.strip_prefix('p').unwrap_or(suffix);
    suffix
        .parse::<u32>()
        .context("Failed to parse partition number")
}

pub fn align_down(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }
    value - (value % alignment)
}
