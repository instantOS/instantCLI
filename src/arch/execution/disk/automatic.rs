use super::util::get_part_path;
use crate::arch::execution::CommandRunner;
use anyhow::Result;
use std::process::Command;

pub fn partition_uefi(disk: &str, executor: &dyn CommandRunner, swap_size_gb: u64) -> Result<()> {
    println!("Partitioning for UEFI...");

    let script = format!(
        "label: gpt\n\
         size=1G, type=U\n\
         size={}G, type=S\n\
         type=L\n",
        swap_size_gb
    );

    executor.run_with_input(Command::new("sfdisk").arg(disk), &script)?;

    if !executor.dry_run() {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

pub fn partition_bios(disk: &str, executor: &dyn CommandRunner, swap_size_gb: u64) -> Result<()> {
    println!("Partitioning for BIOS...");

    let script = format!(
        "label: dos\n\
         size={}G, type=82\n\
         type=83\n",
        swap_size_gb
    );

    executor.run_with_input(Command::new("sfdisk").arg(disk), &script)?;

    if !executor.dry_run() {
        executor.run(Command::new("udevadm").arg("settle"))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    Ok(())
}

pub fn format_uefi(disk: &str, executor: &dyn CommandRunner) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);
    let p3 = get_part_path(disk, 3);

    println!("Formatting partitions...");

    executor.run(Command::new("mkfs.fat").args(["-F32", &p1]))?;
    executor.run(Command::new("mkswap").arg(&p2))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", &p3]))?;

    Ok(())
}

pub fn format_bios(disk: &str, executor: &dyn CommandRunner) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);

    println!("Formatting partitions...");

    executor.run(Command::new("mkswap").arg(&p1))?;
    executor.run(Command::new("mkfs.ext4").args(["-F", &p2]))?;

    Ok(())
}

pub fn mount_uefi(disk: &str, executor: &dyn CommandRunner) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);
    let p3 = get_part_path(disk, 3);

    println!("Mounting partitions...");

    executor.run(Command::new("mount").args([&p3, "/mnt"]))?;
    executor.run(Command::new("mount").args(["--mkdir", &p1, "/mnt/boot"]))?;
    executor.run(Command::new("swapon").arg(&p2))?;

    Ok(())
}

pub fn mount_bios(disk: &str, executor: &dyn CommandRunner) -> Result<()> {
    let p1 = get_part_path(disk, 1);
    let p2 = get_part_path(disk, 2);

    println!("Mounting partitions...");

    executor.run(Command::new("mount").args([&p2, "/mnt"]))?;
    executor.run(Command::new("swapon").arg(&p1))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::arch::execution::mock::MockRunner;
    use crate::arch::execution::CommandRunner;
    use std::process::Command;

    #[test]
    fn test_mock_runner_records_commands() {
        let mock = MockRunner::new();
        mock.run(Command::new("echo").arg("hello")).unwrap();
        mock.run(Command::new("ls").arg("-la")).unwrap();

        let log = mock.command_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], "echo hello");
        assert_eq!(log[1], "ls -la");
    }

    #[test]
    fn test_mock_runner_run_with_input() {
        let mock = MockRunner::new();
        mock.run_with_input(Command::new("sfdisk").arg("/dev/sda"), "label: gpt\n")
            .unwrap();

        let log = mock.command_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("sfdisk /dev/sda"));
        assert!(log[0].contains("label: gpt"));
    }

    #[test]
    fn test_partition_uefi_commands() {
        let mock = crate::arch::execution::mock::MockRunner::new();
        super::partition_uefi("/dev/sda", &mock, 4).unwrap();

        let log = mock.command_log();
        // Should have: sfdisk /dev/sda, udevadm settle
        assert!(log[0].starts_with("sfdisk"));
        assert!(log[0].contains("/dev/sda"));
        assert!(log.iter().any(|c| c.contains("udevadm settle")));
    }

    #[test]
    fn test_format_uefi_commands() {
        let mock = crate::arch::execution::mock::MockRunner::new();
        super::format_uefi("/dev/sda", &mock).unwrap();

        let log = mock.command_log();
        assert!(log.iter().any(|c| c.contains("mkfs.fat") && c.contains("-F32")));
        assert!(log.iter().any(|c| c.contains("mkswap")));
        assert!(log.iter().any(|c| c.contains("mkfs.ext4") && c.contains("-F")));
    }
}
