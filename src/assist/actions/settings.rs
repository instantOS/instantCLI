use anyhow::Result;
use std::process::Command;

/// Open the instantOS settings manager
pub fn open_settings() -> Result<()> {
    let current_exe = std::env::current_exe()?;

    Command::new(&current_exe)
        .arg("settings")
        .arg("--gui")
        .spawn()?;

    Ok(())
}
