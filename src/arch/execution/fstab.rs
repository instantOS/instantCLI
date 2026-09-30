use super::CommandRunner;
use anyhow::{Context, Result};
use std::process::Command;

pub fn generate_fstab(executor: &dyn CommandRunner) -> Result<()> {
    println!("Generating fstab...");

    let output_opt = executor.run_with_output(Command::new("genfstab").arg("-U").arg("/mnt"))?;

    if let Some(output) = output_opt {
        let content = String::from_utf8(output.stdout).context("genfstab output is not UTF-8")?;
        let content = stable_swap_sources(&content, |device| {
            // Probe the device directly: lsblk/udev can still have no UUID for
            // freshly formatted swap, causing genfstab -U to emit /dev/vdb1.
            let output = executor
                .run_with_output(
                    Command::new("blkid").args(["-p", "-s", "UUID", "-o", "value", "--", device]),
                )?
                .context("Swap UUID probe produced no output")?;
            String::from_utf8(output.stdout).context("Swap UUID is not UTF-8")
        })?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/mnt/etc/fstab")?;

        file.write_all(content.as_bytes())?;
    } else {
        // Dry run: we already printed the command in run_with_output
        // We might want to simulate the write?
        println!("[DRY RUN] Writing output to /mnt/etc/fstab");
    }

    println!("Fstab generated.");
    Ok(())
}

fn stable_swap_sources(
    content: &str,
    mut probe_uuid: impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let mut result = String::new();
    for line in content.split_inclusive('\n') {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() >= 3 && fields[2] == "swap" && fields[0].starts_with("/dev/") {
            let uuid = probe_uuid(fields[0])?;
            let uuid = uuid.trim();
            anyhow::ensure!(
                !uuid.is_empty() && uuid.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
                "No valid persistent swap UUID for {}",
                fields[0]
            );
            result.push_str(&line.replacen(fields[0], &format!("UUID={uuid}"), 1));
        } else {
            result.push_str(line);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_survives_target_disk_renumbering() {
        let input =
            "# /dev/vdb1\nUUID=root / ext4 defaults 0 1\n/dev/vdb1\tnone swap defaults 0 0\n";
        let result = stable_swap_sources(input, |device| {
            assert_eq!(device, "/dev/vdb1");
            Ok("abc-123\n".into())
        })
        .unwrap();
        assert_eq!(
            result,
            "# /dev/vdb1\nUUID=root / ext4 defaults 0 1\nUUID=abc-123\tnone swap defaults 0 0\n"
        );
    }

    #[test]
    fn persistent_sources_and_swap_files_are_preserved() {
        let input = "UUID=abc none swap defaults 0 0\n/swapfile none swap defaults 0 0\n/dev/vdb2 / ext4 defaults 0 1\n";
        assert_eq!(
            stable_swap_sources(input, |_| panic!("unexpected probe")).unwrap(),
            input
        );
    }

    #[test]
    fn failed_uuid_probes_do_not_produce_an_unbootable_fstab() {
        let input = "/dev/vdb1 none swap defaults 0 0\n";
        assert!(stable_swap_sources(input, |_| anyhow::bail!("probe failed")).is_err());
        for invalid in ["", "\n", "abc def", "abc\nUUID=other", "../device"] {
            assert!(stable_swap_sources(input, |_| Ok(invalid.into())).is_err());
        }
    }
}
