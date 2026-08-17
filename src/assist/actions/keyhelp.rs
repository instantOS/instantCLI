use anyhow::Result;
use std::process::Command;

/// Open the instantWM keyhelp viewer in a GUI terminal window
pub fn open_keyhelp() -> Result<()> {
    let current_exe = std::env::current_exe()?;

    Command::new(&current_exe)
        .arg("keyhelp")
        .arg("--gui")
        .spawn()?;

    Ok(())
}
