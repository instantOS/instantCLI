use super::CommandExecutor;
use crate::arch::engine::InstallContext;
use anyhow::Result;
use std::process::Command;

pub fn generate_fstab(_context: &InstallContext, executor: &CommandExecutor) -> Result<()> {
    println!("Generating fstab...");

    let output_opt = executor.run_with_output(Command::new("genfstab").arg("-U").arg("/mnt"))?;

    if let Some(output) = output_opt {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/mnt/etc/fstab")?;

        file.write_all(&output.stdout)?;
    } else {
        // Dry run: we already printed the command in run_with_output
        // We might want to simulate the write?
        println!("[DRY RUN] Writing output to /mnt/etc/fstab");
    }

    println!("Fstab generated.");
    Ok(())
}
