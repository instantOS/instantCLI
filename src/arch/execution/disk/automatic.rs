use super::util::get_part_path;
use crate::arch::execution::CommandExecutor;
use anyhow::Result;
use std::process::Command;

pub fn partition_uefi(disk: &str, executor: &CommandExecutor, swap_size_gb: u64) -> Result<()> {
    println!("Partitioning for UEFI...");

    let script = format!(
        "label: gpt\n\
         size=1G, type=U\n\
         size={}G, type=S\n\
         type=L\n",
        swap_size_gb
    );

    executor.run_with_input(Command::new("sfdisk").arg(disk), &script)?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

pub fn partition_bios(disk: &str, executor: &CommandExecutor, swap_size_gb: u64) -> Result<()> {
    println!("Partitioning for BIOS...");

    let script = format!(
        "label: dos\n\
         size={}G, type=82\n\
         type=83\n",
        swap_size_gb
    );

    executor.run_with_input(Command::new("sfdisk").arg(disk), &script)?;

    if !executor.dry_run {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

pub fn format_uefi(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);
    let p3 = get_part_path(disk, 3);

    println!("Formatting partitions...");

    executor.run(Command::new("mkfs.fat").args(["-F32", &p1]))?;
    executor.run(Command::new("mkswap").arg(&p2))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", &p3]))?;

    Ok(())
}

pub fn format_bios(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);

    println!("Formatting partitions...");

    executor.run(Command::new("mkswap").arg(&p1))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", &p2]))?;

    Ok(())
}

pub fn mount_uefi(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);
    let p3 = get_part_path(disk, 3);

    println!("Mounting partitions...");

    executor.run(Command::new("mount").args([&p3, "/mnt"]))?;
    executor.run(Command::new("mount").args(["--mkdir", &p1, "/mnt/boot"]))?;
    executor.run(Command::new("swapon").arg(&p2))?;

    Ok(())
}

pub fn mount_bios(disk: &str, executor: &CommandExecutor) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);

    println!("Mounting partitions...");

    executor.run(Command::new("mount").args([&p2, "/mnt"]))?;
    executor.run(Command::new("swapon").arg(&p1))?;

    Ok(())
}
